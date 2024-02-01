use serde::Deserialize;
use std::collections::BTreeMap;
use std::io;
use std::str::FromStr;

use super::{HostPath, PackageName, PackageNamespace};
use crate::somehow::{Context, LowLevelResult, Result};

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TomlManifest {
    #[serde(default)]
    package_manager: bool,
    #[serde(default)]
    targets: Option<Vec<Target>>,
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

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub arch: Option<String>,
    pub os: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Manifest {
    pub package_manager: bool,
    pub targets: Option<Vec<Target>>,
    pub depends: BTreeMap<PackageNamespace, BTreeMap<PackageName, Dependency>>,
    pub build_depends: BTreeMap<PackageNamespace, BTreeMap<PackageName, Dependency>>,
}

impl Manifest {
    pub fn read(dir_path: &HostPath, path: &str) -> LowLevelResult<Option<Self>> {
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
}

fn parse(buf: &str) -> Result<Manifest> {
    let manifest: TomlManifest = toml::from_str(buf).enough_context()?;
    convert(manifest)
}

fn convert(manifest: TomlManifest) -> Result<Manifest> {
    Ok(Manifest {
        package_manager: manifest.package_manager,
        targets: manifest.targets,
        depends: convert_depends(manifest.depends)?,
        build_depends: convert_depends(manifest.build_depends)?,
    })
}

fn convert_depends(
    deps: BTreeMap<String, DependencyOrTable>,
) -> Result<BTreeMap<PackageNamespace, BTreeMap<PackageName, Dependency>>> {
    let mut map = BTreeMap::new();
    let mut root = BTreeMap::<PackageName, Dependency>::new();
    for (key, value) in deps {
        match value {
            DependencyOrTable::Dependency(dep) => {
                root.insert(PackageName::strict_from_str(&key)?, dep);
            }
            DependencyOrTable::Table(table) => {
                map.insert(PackageNamespace::from_str(&key)?, convert_table(table)?);
            }
        }
    }
    map.insert(PackageNamespace::Root, root);
    Ok(map)
}

fn convert_table(table: BTreeMap<String, Dependency>) -> Result<BTreeMap<PackageName, Dependency>> {
    table
        .into_iter()
        .map(|(name, dep)| Ok((PackageName::loose_from_str(&name)?, dep)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use expect_test::expect;

    #[test]
    fn parse() {
        assert_eq!(
            Manifest {
                package_manager: false,
                targets: None,
                depends: BTreeMap::from([(PackageNamespace::Root, BTreeMap::new())]),
                build_depends: BTreeMap::from([(PackageNamespace::Root, BTreeMap::new())]),
            },
            super::parse("").unwrap()
        );

        expect![[r#"
            Manifest {
                package_manager: true,
                targets: Some(
                    [
                        Target {
                            arch: Some(
                                "x86_64",
                            ),
                            os: Some(
                                "linux",
                            ),
                        },
                    ],
                ),
                depends: {
                    Root: {
                        PackageName(
                            "x",
                        ): Dependency,
                        PackageName(
                            "y",
                        ): Dependency,
                    },
                    Debian: {
                        PackageName(
                            "ca-certificates",
                        ): Dependency,
                    },
                },
                build_depends: {
                    Root: {
                        PackageName(
                            "z",
                        ): Dependency,
                    },
                    Debian: {
                        PackageName(
                            "clang",
                        ): Dependency,
                        PackageName(
                            "cmake",
                        ): Dependency,
                    },
                },
            }
        "#]]
        .assert_debug_eq(
            &super::parse(
                "
                package_manager = true
                [[targets]]
                arch = 'x86_64'
                os = 'linux'
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
                ",
            )
            .unwrap(),
        );
    }
}
