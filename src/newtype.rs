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

mod private_paths {
    use anyhow::{anyhow, Error, Result};
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    /// Defines a type similar to, but incompatible with, a `PathBuf`.
    ///
    /// The goal of this is to reduce confusion about which filesystem
    /// namespace is in question, the host's or the environment's. The getters
    /// should be named differently to make the calling code look wrong when
    /// it's mixing namespaces.
    ///
    /// The resulting type has two key restrictions compared to PathBuf:
    ///
    /// 1. It requires paths to be absolute.
    /// 2. It does not allow joining to absolute paths.
    macro_rules! abs_path {
        ($name:ident, $getter:ident) => {
            #[derive(Debug, Clone)]
            pub struct $name(PathBuf);

            impl $name {
                pub fn $getter(&self) -> &Path {
                    &self.0
                }

                /// Append a relative path to this path.
                ///
                /// Panics if `end` is not a relative path.
                pub fn join<P: AsRef<Path>>(&self, end: P) -> Self {
                    let end = end.as_ref();
                    // TODO: This check is probably broken for weird Windows
                    // paths. See `PathBuf::push` docs.
                    assert!(
                        end.is_relative(),
                        "{} cannot be joined to absolute path, got {:?}",
                        Self::type_name(),
                        end,
                    );
                    Self(self.0.join(end))
                }

                /// Returns `$name` as a string for use in error messages.
                fn type_name() -> &'static str {
                    let name = std::any::type_name::<$name>();
                    match name.rsplit_once("::") {
                        None => name,
                        Some((_, end)) => end,
                    }
                }
            }

            impl TryFrom<PathBuf> for $name {
                type Error = Error;
                fn try_from(p: PathBuf) -> Result<Self> {
                    if p.is_absolute() {
                        Ok(Self(p))
                    } else {
                        Err(anyhow!(
                            "{} must be absolute path, got {p:?}",
                            Self::type_name()
                        ))
                    }
                }
            }

            impl TryFrom<OsString> for $name {
                type Error = Error;
                fn try_from(s: OsString) -> Result<Self> {
                    Self::try_from(PathBuf::from(s))
                }
            }

            impl TryFrom<String> for $name {
                type Error = Error;
                fn try_from(s: String) -> Result<Self> {
                    Self::try_from(PathBuf::from(s))
                }
            }
        };
    }

    abs_path!(HostPath, as_host_raw);
    abs_path!(EnvPath, as_env_raw);
}

pub(crate) use private_paths::{EnvPath, HostPath};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_from_str_relative() {
        assert_eq!(
            "HostPath must be absolute path, got \"hi\"",
            HostPath::try_from(String::from("hi"))
                .unwrap_err()
                .to_string()
        );
    }

    #[test]
    #[should_panic(expected = "EnvPath cannot be joined to absolute path, got \"/bye\"")]
    fn path_join_absolute() {
        EnvPath::try_from(String::from("/hi")).unwrap().join("/bye");
    }
}
