macro_rules! name {
    ($name:ident) => {
        #[derive(Debug)]
        pub struct $name(String);

        impl $name {
            pub fn new(s: String) -> Self {
                Self(s)
            }
        }

        impl std::ops::Deref for $name {
            type Target = str;

            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl AsRef<std::ffi::OsStr> for $name {
            fn as_ref(&self) -> &std::ffi::OsStr {
                self.0.as_ref()
            }
        }
    };
}

pub(crate) use name;
