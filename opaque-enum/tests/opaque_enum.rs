#![allow(missing_docs)]

use opaque_enum::opaque_enum;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io;
use std::sync::Arc;

#[opaque_enum]
#[derive(Debug)]
pub enum DemoError {
    Missing,
    Io(io::Error),
    Named { raw: String },
}

impl From<io::Error> for DemoError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

#[opaque_enum]
impl Display for DemoError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => f.write_str("missing value"),
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Named { raw } => write!(f, "named payload: {raw}"),
        }
    }
}

#[opaque_enum]
impl Error for DemoError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Missing | Self::Named { .. } => None,
        }
    }
}

#[test]
fn unit_constructor_is_function() {
    let err = DemoError::Missing();
    assert_eq!(err.to_string(), "missing value");
}

#[test]
fn tuple_constructor_works_as_from_body() {
    let err = DemoError::from(io::Error::new(io::ErrorKind::NotFound, "gone"));
    assert_eq!(err.to_string(), "io error: gone");
    assert!(err.source().is_some());
}

#[test]
fn named_payload_patterns_are_rewritten_in_impls() {
    let err = DemoError::Named("abc".to_owned());
    assert_eq!(err.to_string(), "named payload: abc");
    assert!(err.source().is_none());
}

#[opaque_enum]
#[derive(Debug)]
enum ReceiverDemo {
    Count(usize),
}

#[opaque_enum]
impl ReceiverDemo {
    fn get(&self) -> usize {
        match self {
            Self::Count(value) => *value,
        }
    }

    fn bump(&mut self) {
        match self {
            Self::Count(value) => *value += 1,
        }
    }

    fn double(self) -> Self {
        match self {
            Self::Count(value) => Self::Count(value * 2),
        }
    }

    // fn droup(&self) -> &Self {
    //     self
    // }
    //
    fn from_arc(self: Arc<Self>) -> usize {
        match self.as_ref() {
            Self::Count(value) => *value,
        }
    }

    fn get_pinned(self: ::std::pin::Pin<&Self>) -> usize {
        match self.get_ref() {
            Self::Count(value) => *value,
        }
    }

    fn bump_pinned(self: ::std::pin::Pin<&mut Self>) {
        match self.get_mut() {
            Self::Count(value) => *value += 1,
        }
    }
}

#[test]
fn forwarded_receivers_project_to_inner_receiver() {
    let mut value = ReceiverDemo::Count(7);
    assert_eq!(value.get(), 7);

    value.bump();
    assert_eq!(value.get(), 8);

    let value = value.double();
    assert_eq!(value.get(), 16);

    assert_eq!(ReceiverDemo::from_arc(Arc::new(value)), 16);
}

#[opaque_enum(wrapper = Box)]
#[derive(Debug)]
enum BoxedDemo {
    Value(String),
}

#[opaque_enum]
impl BoxedDemo {
    fn value(&self) -> &str {
        match self {
            Self::Value(value) => value,
        }
    }
}

#[test]
fn boxed_representation_is_opt_in() {
    let value = BoxedDemo::Value("boxed".to_owned());
    assert_eq!(value.value(), "boxed");
}

#[test]
fn forwarded_pin_receivers_project_correctly() {
    use std::pin::Pin;
    let mut value = ReceiverDemo::Count(5);
    let mut pin_mut = Pin::new(&mut value);
    pin_mut.as_mut().bump_pinned();

    let pin_ref = Pin::new(&value);
    assert_eq!(pin_ref.get_pinned(), 6);
}
