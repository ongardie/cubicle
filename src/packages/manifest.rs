use serde::Deserialize;
use std::collections::BTreeMap;
use std::io;
use std::str::FromStr;

use super::{HostPath, PackageNamespace};
use crate::somehow::{Context, LowLevelResult, Result};

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TomlManifest {
    #[serde(default)]
    depends: BTreeMap<String, DependencyOrTable>,
    #[serde(default)]
    build_depends: BTreeMap<String, DependencyOrTable>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
#[serde(deny_unknown_fields)]
enum DependencyOrTable {
    Dependency(Dependency),
    Table(BTreeMap<String, Dependency>),
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Dependency {}

#[derive(Debug, PartialEq, Eq)]
pub struct Manifest {
    pub depends: BTreeMap<PackageNamespace, BTreeMap<String, Dependency>>,
    pub build_depends: BTreeMap<PackageNamespace, BTreeMap<String, Dependency>>,
}

impl Manifest {
    pub fn read(dir_path: &HostPath, path: &str) -> LowLevelResult<Option<Manifest>> {
        let dir = cap_std::fs::Dir::open_ambient_dir(
            dir_path.as_host_raw(),
            cap_std::ambient_authority(),
        )
        .with_context(|| format!("failed to open directory {:?}", dir_path.as_host_raw()))?;
        let buf = match dir.read_to_string(path) {
            Ok(buf) => buf,
            Err(e) => {
                return if e.kind() == io::ErrorKind::NotFound {
                    Ok(None)
                } else {
                    Err(e)
                        .with_context(|| {
                            format!("failed to read {:?}", dir_path.join(path).as_host_raw())
                        })
                        .map_err(|e| e.into())
                }
            }
        };
        Ok(Some(parse(&buf)?))
    }

    pub fn root_depends(&self) -> &BTreeMap<String, Dependency> {
        self.depends.get(PackageNamespace::root()).unwrap()
    }

    pub fn root_build_depends(&self) -> &BTreeMap<String, Dependency> {
        self.build_depends.get(PackageNamespace::root()).unwrap()
    }
}

fn parse(buf: &str) -> Result<Manifest> {
    let manifest: TomlManifest = toml::from_str(buf).enough_context()?;
    convert(manifest)
}

fn convert(manifest: TomlManifest) -> Result<Manifest> {
    Ok(Manifest {
        depends: convert_depends(manifest.depends)?,
        build_depends: convert_depends(manifest.build_depends)?,
    })
}

fn convert_depends(
    deps: BTreeMap<String, DependencyOrTable>,
) -> Result<BTreeMap<PackageNamespace, BTreeMap<String, Dependency>>> {
    let mut map = BTreeMap::new();
    let mut root = BTreeMap::<String, Dependency>::new();
    for (key, value) in deps {
        match value {
            DependencyOrTable::Dependency(dep) => {
                root.insert(key, dep);
            }
            DependencyOrTable::Table(table) => {
                map.insert(PackageNamespace::from_str(&key)?, table);
            }
        }
    }
    map.insert(PackageNamespace::root_owned(), root);
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_debug_snapshot;

    #[test]
    fn parse() {
        assert_eq!(
            Manifest {
                depends: BTreeMap::from([(PackageNamespace::root_owned(), BTreeMap::new())]),
                build_depends: BTreeMap::from([(PackageNamespace::root_owned(), BTreeMap::new())]),
            },
            super::parse("").unwrap()
        );

        assert_debug_snapshot!(
            super::parse("
                [depends]
                x = {}
                y = {}
                [build_depends]
                z = {}
                [depends.debian]
                ca-certificates = {}
                [build_depends.debian]
                clang = {}
                cmake = {}
                "
            ).unwrap(),
            @r###"
        Manifest {
            depends: {
                PackageNamespace(
                    "cubicle",
                ): {
                    "x": Dependency,
                    "y": Dependency,
                },
                PackageNamespace(
                    "debian",
                ): {
                    "ca-certificates": Dependency,
                },
            },
            build_depends: {
                PackageNamespace(
                    "cubicle",
                ): {
                    "z": Dependency,
                },
                PackageNamespace(
                    "debian",
                ): {
                    "clang": Dependency,
                    "cmake": Dependency,
                },
            },
        }
        "###
        );
    }
}
