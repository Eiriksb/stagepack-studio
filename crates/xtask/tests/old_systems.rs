//! The Linux package, built on Ubuntu 22.04 by `scripts/package-linux.sh`, installed and run on the oldest
//! systems it is for: Ubuntu 22.04 and Debian 12, each in a throwaway container.
//! Run explicitly: `scripts/cargo-safe.sh test -p xtask --test old_systems -- --ignored` (needs docker).
//!
//! The helper isn't asked to start a program here, since the app has no command that does that; the
//! container check runs it with its input closed, which shows the loader accepts it, and
//! `the_staged_app_needs_nothing_else` with `STUDIO_STAGED` drives it fully on the host.

use std::process::Command;
use xtask::linux::{tarball_name, version};
use xtask::stage::repo_root;

/// Each step the container runs, by name; each prints `@@step NAME CODE` when it ends.
const STEPS: [&str; 8] = ["libraries", "unpack", "install", "help", "check", "built", "helper-loads", "uninstall"];

/// What runs inside: the libraries are installed as root, and the rest runs as an ordinary user
/// (`install.sh` refuses root) with the scratch home `/tmp/h`. Nothing on the host is written.
fn script(top: &str, tarball: &str) -> String {
    format!(
        r#"set -u
step() {{ _name=$1; shift; "$@"; echo "@@step $_name $?"; }}
export DEBIAN_FRONTEND=noninteractive PATH=/usr/sbin:/usr/bin:/sbin:/bin LANG=C.UTF-8
libraries() {{
  apt-get update -qq && apt-get install -y -qq --no-install-recommends \
    zstd libxkbcommon0 libxkbcommon-x11-0 libxcb1 libstdc++6 libfontconfig1 libfreetype6 libwayland-client0 >/dev/null
}}
step libraries libraries
useradd -M -d /tmp/h -s /bin/sh tester && mkdir -p /tmp/h && chown tester: /tmp/h
cat > /tmp/as-user.sh <<'END'
set -u
step() {{ _name=$1; shift; "$@"; echo "@@step $_name $?"; }}
export HOME=/tmp/h PATH=/usr/bin:/bin LANG=C.UTF-8
mkdir -p /tmp/h/unpack
step unpack tar -I zstd -xf /pkg/{tarball} -C /tmp/h/unpack
cd /tmp/h/unpack/{top}
step install sh install.sh
cd /tmp/h
step help /tmp/h/.local/bin/stagepack-studio help
cp -R /fixtures/components /tmp/h/talk && rm -rf /tmp/h/talk/build
mkdir -p /tmp/h/.local/share/stagepack-studio
printf '{{"always_build":["/tmp/h/talk"]}}' > /tmp/h/.local/share/stagepack-studio/build-permissions.json
step check /tmp/h/.local/bin/stagepack-studio check /tmp/h/talk
step built test -d /tmp/h/talk/build
step helper-loads sh -c '/tmp/h/.local/lib/stagepack-studio/stagepack-terminal </dev/null'
step uninstall sh /tmp/h/.local/lib/stagepack-studio/uninstall.sh
END
su tester -s /bin/sh -c 'sh /tmp/as-user.sh'
"#
    )
}

#[test]
#[ignore = "needs docker and the package from scripts/package-linux.sh; run by test-all"]
fn the_package_runs_on_ubuntu_22_04_and_debian_12() {
    let repo = repo_root();
    let version = version().unwrap();
    let tarball = tarball_name(&version);
    let package = repo.join("target/package");
    assert!(package.join(&tarball).is_file(), "{} is missing: run scripts/package-linux.sh first", package.join(&tarball).display());
    let script = script(&format!("stagepack-studio-{version}"), &tarball);

    let mut failures = Vec::new();
    for (image, name) in [("ubuntu:22.04", "stagepack-studio-old-ubuntu-22.04"), ("debian:12", "stagepack-studio-old-debian-12")] {
        let output = Command::new("docker")
            .args(["run", "--rm", "--name", name, "-v"])
            .arg(format!("{}:/pkg:ro", package.display()))
            .arg("-v")
            .arg(format!("{}:/fixtures/components:ro", repo.join("fixtures/components").display()))
            .args([image, "sh", "-c", &script])
            .output()
            .expect("docker runs");
        let said = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let codes: Vec<(String, String)> = said
            .lines()
            .filter_map(|line| {
                let mut parts = line.strip_prefix("@@step ")?.split(' ');
                Some((parts.next()?.to_string(), parts.next()?.to_string()))
            })
            .collect();
        eprintln!("{image}: {codes:?}");
        for step in STEPS {
            match codes.iter().find(|(name, _)| name == step) {
                Some((_, code)) if code == "0" => {}
                Some((_, code)) => failures.push(format!("{image}: {step} exited {code}")),
                None => failures.push(format!("{image}: {step} never ran")),
            }
        }
        if !output.status.success() || codes.iter().any(|(_, code)| code != "0") || codes.len() != STEPS.len() {
            eprintln!("---- {image} said:\n{said}");
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
