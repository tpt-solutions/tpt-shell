// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Security boundaries for consent-gated path grants, on the real file system. The unit tests
//! cover the lexical rules and the on-disk check separately; these tests cover the combination
//! a page actually meets: a grant that asks the user, then a path that leaves the root through a
//! link. Each test names the rule it checks.

use std::fs;
use std::path::{Path, PathBuf};

use tpt_shell_capabilities::{ConsentPrompt, Decision, Engine, Grant, Scope};

/// Answers every question with a fixed answer and counts the questions it was asked.
struct FixedPrompt {
    answer: bool,
    asked: u32,
}

impl ConsentPrompt for FixedPrompt {
    fn ask(&mut self, _window: &str, _capability: &str) -> bool {
        self.asked += 1;
        self.answer
    }
}

/// A fresh scratch folder for one test, removed at the start of the run.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tpt-sec-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch folder");
    dir
}

/// Makes `link` a directory link to `target`. On Windows a directory junction is the fallback
/// when the symbolic-link privilege is missing; it needs no privilege and the engine follows it
/// the same way. Returns false when neither can be made, so the caller can skip.
fn link_dir(target: &Path, link: &Path) -> bool {
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return true;
        }
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output();
        match made {
            Ok(out) if out.status.success() => true,
            _ => {
                eprintln!("skipped: cannot create a link here (no symlink privilege, no junction)");
                false
            }
        }
    }
    #[cfg(unix)]
    {
        match std::os::unix::fs::symlink(target, link) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("skipped: cannot create a symbolic link here ({e})");
                false
            }
        }
    }
}

fn consenting_grant(root: &Path) -> Grant {
    Grant {
        capability: "fs.read".into(),
        window: "main".into(),
        origin: "https://tpt-app.localhost".into(),
        scope: Scope::Paths(vec![root.to_string_lossy().into_owned()]),
        consent: true,
    }
}

#[test]
fn a_link_out_of_the_root_is_denied_even_after_the_user_consents() {
    let base = scratch("consent-link");
    let root = base.join("data");
    let outside = base.join("outside");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), b"not for the page").unwrap();
    if !link_dir(&outside, &root.join("link")) {
        return;
    }

    let mut engine = Engine::new();
    engine.add_grant(consenting_grant(&root)).unwrap();
    let mut prompt = FixedPrompt {
        answer: true,
        asked: 0,
    };
    let escaped = root.join("link").join("secret.txt");
    let decision = engine.authorize_path_on_disk(
        &mut prompt,
        "main",
        "https://tpt-app.localhost",
        "fs.read",
        &escaped.to_string_lossy(),
    );

    // The user was asked, because the path looks inside the root until it is resolved. Their
    // yes must not reach a file outside the root.
    assert_eq!(prompt.asked, 1);
    assert!(
        matches!(decision, Decision::Deny(_)),
        "a link out of the root was allowed: {decision:?}"
    );
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn a_file_inside_the_root_is_allowed_after_consent() {
    // The positive case for the test above: the consent path works when nothing escapes.
    let base = scratch("consent-inside");
    let root = base.join("data");
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("sub").join("report.txt"), b"ok").unwrap();

    let mut engine = Engine::new();
    engine.add_grant(consenting_grant(&root)).unwrap();
    let mut prompt = FixedPrompt {
        answer: true,
        asked: 0,
    };
    let inside = root.join("sub").join("report.txt");
    let decision = engine.authorize_path_on_disk(
        &mut prompt,
        "main",
        "https://tpt-app.localhost",
        "fs.read",
        &inside.to_string_lossy(),
    );

    assert_eq!(prompt.asked, 1);
    assert_eq!(decision, Decision::Allow);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn a_declined_consent_keeps_an_escape_refused_and_is_not_asked_again() {
    // A "no" is remembered: a later request for the same capability in the same window is refused
    // without asking again, whichever path it names.
    let base = scratch("consent-declined");
    let root = base.join("data");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("note.txt"), b"hi").unwrap();

    let mut engine = Engine::new();
    engine.add_grant(consenting_grant(&root)).unwrap();
    let mut prompt = FixedPrompt {
        answer: false,
        asked: 0,
    };
    let inside = root.join("note.txt");
    let first = engine.authorize_path_on_disk(
        &mut prompt,
        "main",
        "https://tpt-app.localhost",
        "fs.read",
        &inside.to_string_lossy(),
    );
    let second = engine.authorize_path_on_disk(
        &mut prompt,
        "main",
        "https://tpt-app.localhost",
        "fs.read",
        &inside.to_string_lossy(),
    );

    assert!(matches!(first, Decision::Deny(_)));
    assert!(matches!(second, Decision::Deny(_)));
    assert_eq!(prompt.asked, 1, "a declined answer is remembered");
    let _ = fs::remove_dir_all(&base);
}
