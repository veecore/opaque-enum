//! Proc macro implementation for `opaque-enum`.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{
    Attribute, Fields, FnArg, Ident, ImplItem, ImplItemFn, Item, ItemEnum, ItemImpl, LifetimeParam,
    Pat, Path, ReturnType, Token, Type, TypePath, Visibility, parse_quote,
};

/// Hides enum variants behind an opaque struct wrapper.
///
/// This macro lets a public type keep an enum-like authoring experience while
/// exposing an opaque wrapper instead of public enum variants. This prevents
/// breaking changes when you add, remove, or modify variants.
///
/// # Examples
///
/// ```ignore
/// # use opaque_enum_macros::opaque_enum;
/// use std::fmt::{self, Display, Formatter};
///
/// #[opaque_enum]
/// #[derive(Debug)]
/// pub enum DatabaseError {
///     ConnectionFailed(String),
///     QueryFailed { query: String, reason: String },
///     PermissionDenied,
/// }
///
/// #[opaque_enum]
/// impl Display for DatabaseError {
///     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
///         match self {
///             Self::ConnectionFailed(err) => write!(f, "connection failed: {err}"),
///             Self::QueryFailed { query, reason } => {
///                 write!(f, "query `{query}` failed: {reason}")
///             }
///             Self::PermissionDenied => write!(f, "permission denied"),
///         }
///     }
/// }
/// ```
///
/// You can also opt-in to boxing the representation by specifying `wrapper = Box`:
///
/// ```ignore
/// # use opaque_enum_macros::opaque_enum;
/// #[opaque_enum(wrapper = Box)]
/// #[derive(Debug)]
/// pub enum LargeError {
///     Variant1([u8; 1024]),
///     Variant2,
/// }
/// ```
#[proc_macro_attribute]
pub fn opaque_enum(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = match syn::parse::<OpaqueArgs>(attr) {
        Ok(args) => args,
        Err(err) => return err.to_compile_error().into(),
    };

    match syn::parse::<Item>(item) {
        Ok(Item::Enum(item_enum)) => expand_enum(args, item_enum).into(),
        Ok(Item::Impl(item_impl)) => expand_impl(item_impl).into(),
        Ok(other) => syn::Error::new_spanned(
            other,
            "`#[opaque_enum]` can only be applied to enums and impl blocks",
        )
        .to_compile_error()
        .into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Storage {
    Inline,
    Boxed,
}

#[derive(Clone, Copy, Debug)]
struct OpaqueArgs {
    storage: Storage,
}

impl Parse for OpaqueArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(Self {
                storage: Storage::Inline,
            });
        }

        let key: Ident = input.parse()?;
        if key != "wrapper" {
            return Err(syn::Error::new_spanned(
                key,
                "expected `wrapper = Box` or no arguments",
            ));
        }

        input.parse::<Token![=]>()?;
        let value: Ident = input.parse()?;
        if value != "Box" {
            return Err(syn::Error::new_spanned(
                value,
                "only `wrapper = Box` is currently supported",
            ));
        }

        if !input.is_empty() {
            input.parse::<Token![,]>()?;
            if !input.is_empty() {
                return Err(input.error("unexpected extra opaque_enum arguments"));
            }
        }

        Ok(Self {
            storage: Storage::Boxed,
        })
    }
}

fn expand_enum(args: OpaqueArgs, item: ItemEnum) -> proc_macro2::TokenStream {
    let ItemEnum {
        attrs,
        vis,
        ident,
        generics,
        variants,
        ..
    } = item;
    let inner_ident = inner_ident(&ident);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let constructor_vis = constructor_vis(&vis);
    let constructors = variants
        .iter()
        .map(|variant| constructor(&constructor_vis, &ident, &inner_ident, variant));
    let public_attrs = public_attrs(&attrs);
    let storage_field = storage_field(args.storage, &inner_ident, &ty_generics);
    let from_body = from_body(args.storage);
    let into_inner_body = into_inner_body(args.storage);
    let as_inner_body = as_inner_body(args.storage);
    let as_inner_mut_body = as_inner_mut_body(args.storage);
    let projection_impls = projection_impls(args.storage, &ident, &inner_ident, &generics);
    let repr = (args.storage == Storage::Inline).then(|| quote!(#[repr(transparent)]));

    quote! {
        #repr
        #(#public_attrs)*
        #vis struct #ident #generics #where_clause {
            inner: #storage_field,
        }

        #(#attrs)*
        enum #inner_ident #generics #where_clause {
            #variants
        }

        impl #impl_generics #ident #ty_generics #where_clause {
            #(#constructors)*

            #[doc(hidden)]
            fn __opaque_into_inner(self) -> #inner_ident #ty_generics {
                #into_inner_body
            }

            #[doc(hidden)]
            fn __opaque_as_inner(&self) -> &#inner_ident #ty_generics {
                #as_inner_body
            }

            #[doc(hidden)]
            fn __opaque_as_inner_mut(&mut self) -> &mut #inner_ident #ty_generics {
                #as_inner_mut_body
            }
        }

        impl #impl_generics ::std::convert::From<#inner_ident #ty_generics>
            for #ident #ty_generics
            #where_clause
        {
            fn from(inner: #inner_ident #ty_generics) -> Self {
                #from_body
            }
        }

        #projection_impls
    }
}

fn projection_impls(
    storage: Storage,
    ident: &Ident,
    inner_ident: &Ident,
    generics: &syn::Generics,
) -> proc_macro2::TokenStream {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let mut ref_generics = generics.clone();
    ref_generics.params.insert(
        0,
        syn::GenericParam::Lifetime(LifetimeParam::new(parse_quote!('__opaque))),
    );
    let (ref_impl_generics, _, ref_where_clause) = ref_generics.split_for_impl();

    let container_impls = (storage == Storage::Inline).then(|| {
        quote! {
            impl #impl_generics ::opaque_enum::OpaqueProject<#inner_ident #ty_generics>
                for ::std::sync::Arc<#ident #ty_generics>
                #where_clause
            {
                type Output = ::std::sync::Arc<#inner_ident #ty_generics>;

                fn project(self) -> Self::Output {
                    let ptr = ::std::sync::Arc::into_raw(self);
                    // SAFETY: inline `#[opaque_enum]` emits a transparent
                    // wrapper over the inner enum and implements
                    // `OpaqueTransparent` for the wrapper.
                    unsafe { ::std::sync::Arc::from_raw(ptr.cast::<#inner_ident #ty_generics>()) }
                }
            }

            impl #impl_generics ::opaque_enum::OpaqueProject<#inner_ident #ty_generics>
                for ::std::rc::Rc<#ident #ty_generics>
                #where_clause
            {
                type Output = ::std::rc::Rc<#inner_ident #ty_generics>;

                fn project(self) -> Self::Output {
                    let ptr = ::std::rc::Rc::into_raw(self);
                    // SAFETY: see the analogous `Arc` implementation above.
                    unsafe { ::std::rc::Rc::from_raw(ptr.cast::<#inner_ident #ty_generics>()) }
                }
            }
        }
    });

    quote! {
        impl #impl_generics ::opaque_enum::OpaqueProject<#inner_ident #ty_generics>
            for #ident #ty_generics
            #where_clause
        {
            type Output = #inner_ident #ty_generics;

            fn project(self) -> Self::Output {
                self.__opaque_into_inner()
            }
        }

        impl #ref_impl_generics ::opaque_enum::OpaqueProject<#inner_ident #ty_generics>
            for &'__opaque #ident #ty_generics
            #ref_where_clause
        {
            type Output = &'__opaque #inner_ident #ty_generics;

            fn project(self) -> Self::Output {
                self.__opaque_as_inner()
            }
        }

        impl #ref_impl_generics ::opaque_enum::OpaqueProject<#inner_ident #ty_generics>
            for &'__opaque mut #ident #ty_generics
            #ref_where_clause
        {
            type Output = &'__opaque mut #inner_ident #ty_generics;

            fn project(self) -> Self::Output {
                self.__opaque_as_inner_mut()
            }
        }

        impl #ref_impl_generics ::opaque_enum::OpaqueProject<#inner_ident #ty_generics>
            for ::std::pin::Pin<&'__opaque #ident #ty_generics>
            #ref_where_clause
        {
            type Output = ::std::pin::Pin<&'__opaque #inner_ident #ty_generics>;

            fn project(self) -> Self::Output {
                // SAFETY: pinning is structurally transparent for immutable references.
                unsafe { self.map_unchecked(|wrapper| wrapper.__opaque_as_inner()) }
            }
        }

        impl #ref_impl_generics ::opaque_enum::OpaqueProject<#inner_ident #ty_generics>
            for ::std::pin::Pin<&'__opaque mut #ident #ty_generics>
            #ref_where_clause
        {
            type Output = ::std::pin::Pin<&'__opaque mut #inner_ident #ty_generics>;

            fn project(self) -> Self::Output {
                // SAFETY: wrapper struct is transparent or boxes the inner type,
                // preserving pinning guarantees.
                unsafe { self.map_unchecked_mut(|wrapper| wrapper.__opaque_as_inner_mut()) }
            }
        }

        #container_impls
    }
}

fn storage_field(
    storage: Storage,
    inner_ident: &Ident,
    ty_generics: &syn::TypeGenerics<'_>,
) -> proc_macro2::TokenStream {
    match storage {
        Storage::Inline => quote!(#inner_ident #ty_generics),
        Storage::Boxed => quote!(::std::boxed::Box<#inner_ident #ty_generics>),
    }
}

fn from_body(storage: Storage) -> proc_macro2::TokenStream {
    match storage {
        Storage::Inline => quote!(Self { inner }),
        Storage::Boxed => quote!(Self {
            inner: ::std::boxed::Box::new(inner)
        }),
    }
}

fn into_inner_body(storage: Storage) -> proc_macro2::TokenStream {
    match storage {
        Storage::Inline => quote!(self.inner),
        Storage::Boxed => quote!(*self.inner),
    }
}

fn as_inner_body(storage: Storage) -> proc_macro2::TokenStream {
    match storage {
        Storage::Inline => quote!(&self.inner),
        Storage::Boxed => quote!(self.inner.as_ref()),
    }
}

fn as_inner_mut_body(storage: Storage) -> proc_macro2::TokenStream {
    match storage {
        Storage::Inline => quote!(&mut self.inner),
        Storage::Boxed => quote!(self.inner.as_mut()),
    }
}

fn constructor_vis(public_vis: &Visibility) -> Visibility {
    match public_vis {
        Visibility::Public(_) => parse_quote!(pub(crate)),
        // TODO
        other => other.clone(),
    }
}

fn constructor(
    vis: &Visibility,
    public_ident: &Ident,
    inner_ident: &Ident,
    variant: &syn::Variant,
) -> proc_macro2::TokenStream {
    let variant_ident = &variant.ident;
    let attrs = doc_attrs(&variant.attrs);

    match &variant.fields {
        Fields::Unit => {
            quote! {
                #(#attrs)*
                #[allow(non_snake_case)]
                #vis fn #variant_ident() -> Self {
                    #public_ident::from(#inner_ident::#variant_ident)
                }
            }
        }
        Fields::Unnamed(fields) => {
            let args = fields.unnamed.iter().enumerate().map(|(index, field)| {
                let ident = format_ident!("field_{index}");
                let ty = &field.ty;
                quote!(#ident: #ty)
            });
            let values = (0..fields.unnamed.len()).map(|index| format_ident!("field_{index}"));
            quote! {
                #(#attrs)*
                #[allow(non_snake_case)]
                #vis fn #variant_ident(#(#args),*) -> Self {
                    #public_ident::from(#inner_ident::#variant_ident(#(#values),*))
                }
            }
        }
        Fields::Named(fields) => {
            let args = fields.named.iter().map(|field| {
                let ident = field.ident.as_ref().expect("named field has an ident");
                let ty = &field.ty;
                quote!(#ident: #ty)
            });
            let values = fields
                .named
                .iter()
                .map(|field| field.ident.as_ref().expect("named field has an ident"));
            quote! {
                #(#attrs)*
                #[allow(non_snake_case)]
                #vis fn #variant_ident(#(#args),*) -> Self {
                    #public_ident::from(#inner_ident::#variant_ident { #(#values),* })
                }
            }
        }
    }
}

#[allow(clippy::single_match_else)]
fn expand_impl(item: ItemImpl) -> proc_macro2::TokenStream {
    let Some(self_type_path) = self_type_path(&item.self_ty) else {
        return syn::Error::new_spanned(
            item.self_ty,
            "`#[opaque_enum]` impl target must be a plain type path",
        )
        .to_compile_error();
    };

    let inner_ty = inner_ty(self_type_path);
    let inner_impl = inner_impl(&item, &inner_ty);

    let wrappers = match item
        .items
        .iter()
        .map(|impl_item| wrapper_item(item.trait_.as_ref(), &inner_ty, impl_item))
        .collect::<syn::Result<Vec<_>>>()
    {
        Ok(wrappers) => wrappers,
        Err(err) => return err.to_compile_error(),
    };

    let attrs = &item.attrs;
    let defaultness = &item.defaultness;
    let unsafety = &item.unsafety;
    let impl_token = &item.impl_token;
    let generics = &item.generics;
    let self_ty = &item.self_ty;
    let public_impl = match &item.trait_ {
        Some((bang, trait_path, for_token)) => quote! {
            #(#attrs)*
            #defaultness #unsafety #impl_token #generics #bang #trait_path #for_token #self_ty {
                #(#wrappers)*
            }
        },
        None => quote! {
            #(#attrs)*
            #defaultness #unsafety #impl_token #generics #self_ty {
                #(#wrappers)*
            }
        },
    };

    quote! {
        #public_impl
        #inner_impl
    }
}

fn wrapper_item(
    trait_: Option<&(Option<Token![!]>, Path, Token![for])>,
    inner_ty: &Type,
    item: &ImplItem,
) -> syn::Result<proc_macro2::TokenStream> {
    let ImplItem::Fn(function) = item else {
        return Err(syn::Error::new_spanned(
            item,
            "`#[opaque_enum]` impl blocks currently support methods only",
        ));
    };
    wrapper_fn(trait_, inner_ty, function)
}

fn wrapper_fn(
    trait_: Option<&(Option<Token![!]>, Path, Token![for])>,
    inner_ty: &Type,
    function: &ImplItemFn,
) -> syn::Result<proc_macro2::TokenStream> {
    if function.sig.asyncness.is_some() {
        return Err(syn::Error::new_spanned(
            function.sig.asyncness,
            "`#[opaque_enum]` does not yet support async methods",
        ));
    }
    if function.sig.constness.is_some() {
        return Err(syn::Error::new_spanned(
            function.sig.constness,
            "`#[opaque_enum]` does not yet support const methods",
        ));
    }

    let attrs = &function.attrs;
    let vis = &function.vis;
    let defaultness = &function.defaultness;
    let sig = &function.sig;
    let method = &function.sig.ident;
    let args = function_args(function)?;
    let receiver = has_receiver(function);
    let call = inner_call(trait_, inner_ty, method, receiver, &args);
    // NOTE: only a bare `-> Self` return is detected and wrapped with `Into::into`.
    // Methods returning composite types that *contain* `Self` (e.g. `Option<Self>`,
    // `Result<Self, E>`) are not rewritten and will produce a type-mismatch compile
    // error. An `InverseProject` trait is planned to handle those cases.
    let body = if returns_self(&function.sig.output) {
        quote!({
            ::std::convert::Into::into(#call)
        })
    } else {
        quote!({
            #call
        })
    };

    Ok(quote! {
        #(#attrs)*
        #defaultness #vis #sig #body
    })
}

fn inner_call(
    trait_: Option<&(Option<Token![!]>, Path, Token![for])>,
    inner_ty: &Type,
    method: &Ident,
    receiver: bool,
    args: &[Ident],
) -> proc_macro2::TokenStream {
    let mut call_args = Vec::new();
    if receiver {
        call_args.push(quote!(
            ::opaque_enum::OpaqueProject::<#inner_ty>::project(self)
        ));
    }
    call_args.extend(args.iter().map(|arg| quote!(#arg)));

    match trait_ {
        Some((_, trait_path, _)) => {
            quote!(<#inner_ty as #trait_path>::#method(#(#call_args),*))
        }
        None => {
            quote!(<#inner_ty>::#method(#(#call_args),*))
        }
    }
}

fn function_args(function: &ImplItemFn) -> syn::Result<Vec<Ident>> {
    function
        .sig
        .inputs
        .iter()
        .filter_map(|arg| match arg {
            FnArg::Receiver(_) => None,
            FnArg::Typed(arg) => Some(arg),
        })
        .map(|arg| match arg.pat.as_ref() {
            Pat::Ident(pat_ident) => Ok(pat_ident.ident.clone()),
            _ => Err(syn::Error::new_spanned(
                &arg.pat,
                "`#[opaque_enum]` forwarding requires simple identifier arguments",
            )),
        })
        .collect()
}

fn has_receiver(function: &ImplItemFn) -> bool {
    matches!(function.sig.inputs.first(), Some(FnArg::Receiver(_)))
}

// Returns true only for a bare `-> Self`. References (`-> &Self`, `-> &mut Self`)
// and composite types (`-> Option<Self>`) are intentionally excluded: wrapping
// references requires transmuting the pointer (only sound for inline storage), and
// wrapping composites requires a not-yet-implemented `InverseProject` pass.
fn returns_self(output: &ReturnType) -> bool {
    matches!(output, ReturnType::Type(_, ty) if type_is_self(ty))
}

fn type_is_self(ty: &Type) -> bool {
    matches!(ty, Type::Path(type_path) if type_path.path.is_ident("Self"))
}

fn public_attrs(attrs: &[Attribute]) -> Vec<&Attribute> {
    attrs
        .iter()
        .filter(|attr| !attr.path().is_ident("repr"))
        .collect()
}

fn doc_attrs(attrs: &[Attribute]) -> Vec<&Attribute> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .collect()
}

fn self_type_path(ty: &Type) -> Option<&TypePath> {
    if let Type::Path(type_path) = ty {
        Some(type_path)
    } else {
        None
    }
}

// Repoints the impl block's `Self` type to the inner enum type. This means all
// `self.method()` calls inside the decorated block are resolved against the inner
// enum, not the public wrapper. As a result, only methods defined in other
// `#[opaque_enum]`-decorated `impl` blocks are callable on `self`; methods
// defined solely on the outer wrapper type are not in scope here.
fn inner_impl(item_impl: &ItemImpl, inner_ty: &Type) -> ItemImpl {
    let mut inner_impl = item_impl.clone();
    *inner_impl.self_ty = inner_ty.clone();
    inner_impl
}

fn inner_ty(type_path: &TypePath) -> Type {
    let mut type_path = type_path.clone();
    let self_ident = &mut type_path.path.segments.last_mut().unwrap().ident;

    let inner_ident = inner_ident(self_ident);

    *self_ident = inner_ident;

    Type::Path(type_path)
}

fn inner_ident(ident: &Ident) -> Ident {
    format_ident!("{ident}Inner")
}
