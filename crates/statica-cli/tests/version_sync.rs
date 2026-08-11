use std::{fs, path::Path};

use serde_json::Value;

#[test]
fn npm_packages_match_cli_version() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let version = env!("CARGO_PKG_VERSION");
    let packages = [
        "npm/@statica/cli/package.json",
        "npm/@statica/cli-darwin-arm64/package.json",
        "npm/@statica/cli-darwin-x64/package.json",
        "npm/@statica/cli-linux-arm64-gnu/package.json",
        "npm/@statica/cli-linux-x64-gnu/package.json",
        "npm/@statica/cli-win32-x64/package.json",
        "npm/create-statica/package.json",
    ];

    for package in packages {
        let path = repo.join(package);
        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read npm package manifest"))
                .expect("parse npm package manifest");

        assert_eq!(
            manifest["version"].as_str(),
            Some(version),
            "{package} version must match statica-cli"
        );

        if let Some(optional_dependencies) = manifest["optionalDependencies"].as_object() {
            for (name, pinned_version) in optional_dependencies {
                assert_eq!(
                    pinned_version.as_str(),
                    Some(version),
                    "{package} optional dependency {name} must match statica-cli"
                );
            }
        }

        if let Some(dependencies) = manifest["dependencies"].as_object() {
            for (name, pinned_version) in dependencies {
                if name.starts_with("@statica/") {
                    assert_eq!(
                        pinned_version.as_str(),
                        Some(version),
                        "{package} dependency {name} must match statica-cli"
                    );
                }
            }
        }
    }
}
