use super::*;

use crate::tests::workspace_mocker::{
    example_workspace_1, example_workspace_2, example_workspace_3,
};
use enumflags2::make_bitflags;
use std::str::FromStr;

#[ctor::ctor]
fn init_logger() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .is_test(true)
        .init();
}

#[test]
fn detect_changed_crates() {
    let workspace_mocker = example_workspace_1().unwrap();
    workspace_mocker.add_or_replace_file(
        "README",
        r#"# Example

            Some changes
        "#,
    );
    let before = workspace_mocker.head().unwrap();
    let after = workspace_mocker.commit(None);

    let workspace = ReleaseWorkspace::try_new(workspace_mocker.root()).unwrap();

    assert_eq!(
        vec![PathBuf::from(workspace.root()).join("README")],
        changed_files(workspace.root(), &before, &after).unwrap()
    );
}

#[test]
fn workspace_members() {
    let workspace_mocker = example_workspace_1().unwrap();
    let workspace = ReleaseWorkspace::try_new(workspace_mocker.root()).unwrap();

    let result = workspace
        .members()
        .unwrap()
        .iter()
        .map(|crt| crt.name())
        .collect::<HashSet<_>>();

    let expected_result = [
        "crate_g", "crate_a", "crate_b", "crate_c", "crate_e", "crate_f",
    ]
    .iter()
    .map(std::string::ToString::to_string)
    .collect::<HashSet<_>>();

    assert_eq!(expected_result, result);
}

#[test]
fn release_selection() {
    let criteria = SelectionCriteria {
        match_filter: fancy_regex::Regex::new("crate_(b|a|e)").unwrap(),
        disallowed_version_reqs: vec![semver::VersionReq::from_str(">=0.1.0").unwrap()],
        allowed_dev_dependency_blockers: make_bitflags!(CrateStateFlags::{MissingReadme}),
        allowed_selection_blockers: make_bitflags!(CrateStateFlags::{MissingReadme}),

        ..Default::default()
    };

    let workspace_mocker = example_workspace_1().unwrap();
    let workspace =
        ReleaseWorkspace::try_new_with_criteria(workspace_mocker.root(), criteria).unwrap();

    // TODO: verify that a crate can be selected by being an unmatched, changed crate that's a dependency of a matched, unchanged crate.

    let selection = workspace
        .release_selection()
        .unwrap()
        .into_iter()
        .map(|c| c.name())
        .collect::<Vec<_>>();
    let expected_selection = vec!["crate_g", "crate_b", "crate_a", "crate_e"];

    assert_eq!(expected_selection, selection);
}

#[test]
fn members_dependencies() {
    let workspace_mocker = example_workspace_2().unwrap();
    let workspace = ReleaseWorkspace::try_new_with_criteria(
        workspace_mocker.root(),
        SelectionCriteria {
            exclude_optional_deps: true,
            ..Default::default()
        },
    )
    .unwrap();

    let result = workspace
        .members()
        .unwrap()
        .iter()
        .map(|crt| {
            (
                crt.name(),
                crt.dependencies_in_workspace()
                    .unwrap()
                    .into_iter()
                    .map(|(dep_name, _)| dep_name.clone())
                    .collect::<HashSet<_>>(),
            )
        })
        .collect::<Vec<_>>();

    let expected_result = [
        ("crate_b".to_string(), vec![]),
        ("crate_c".to_string(), vec!["crate_b".to_string()]),
        (
            "crate_a".to_string(),
            vec!["crate_c".to_string(), "crate_b".to_string()],
        ),
        (
            "crate_d".to_string(),
            vec![
                "crate_a".to_string(),
                "crate_c".to_string(),
                "crate_b".to_string(),
            ],
        ),
    ]
    .into_iter()
    .map(|(name, deps)| (name, deps.into_iter().collect::<HashSet<_>>()))
    .collect::<Vec<_>>();

    pretty_assertions::assert_eq!(expected_result, result, "left is expected");
}

#[test]
fn crate_dependants_unfiltered_and_filtered() {
    let workspace_mocker = example_workspace_2().unwrap();
    let workspace = ReleaseWorkspace::try_new_with_criteria(
        workspace_mocker.root(),
        SelectionCriteria {
            exclude_optional_deps: true,
            ..Default::default()
        },
    )
    .unwrap();

    let crate_b = workspace
        .members()
        .unwrap()
        .iter()
        .find(|crt| crt.name() == "crate_b")
        .unwrap();

    // get something back
    pretty_assertions::assert_ne!(
        Vec::<&Crate>::new(),
        crate_b.dependants_in_workspace_filtered(|_| true).unwrap()
    );

    // unfiltered equals the 'true' filter
    pretty_assertions::assert_eq!(
        crate_b.dependants_in_workspace().unwrap(),
        &crate_b.dependants_in_workspace_filtered(|_| true).unwrap()
    );

    // filter changes work
    //
    // The actual values in here aren't that important, just need to see that the filter is applied and not
    // cached as `true` from above.
    pretty_assertions::assert_eq!(
        Vec::<&Crate>::new(),
        crate_b.dependants_in_workspace_filtered(|_| false).unwrap()
    );

    // dependency doesn't include itself
    pretty_assertions::assert_eq!(
        None,
        crate_b
            .dependants_in_workspace()
            .unwrap()
            .into_iter()
            .find(|crt| crt.name() == "crate_b")
    );

    // for the sake of completeness exhaustively check the result
    pretty_assertions::assert_eq!(
        vec!["crate_c", "crate_a", "crate_d"],
        crate_b
            .dependants_in_workspace()
            .unwrap()
            .iter()
            .map(|crt| crt.name())
            .collect::<Vec<_>>(),
    );
}

#[test]
fn members_sorted_ws1() {
    let workspace_mocker = example_workspace_1().unwrap();
    let workspace = ReleaseWorkspace::try_new_with_criteria(
        workspace_mocker.root(),
        SelectionCriteria {
            allowed_dev_dependency_blockers: (&[
                IsWorkspaceDependency,
                UnreleasableViaChangelogFrontmatter,
                MissingChangelog,
            ] as &[CrateStateFlags])
                .iter()
                .cloned()
                .collect(),

            ..Default::default()
        },
    )
    .unwrap();

    let result = workspace
        .members()
        .unwrap()
        .iter()
        .map(|crt| crt.name())
        .collect::<Vec<_>>();

    let expected_result = [
        "crate_g", "crate_b", "crate_a", "crate_c", "crate_e", "crate_f",
    ]
    .iter()
    .map(std::string::ToString::to_string)
    .collect::<Vec<_>>();

    assert_eq!(expected_result, result);
}

#[test]
fn members_sorted_ws2() {
    let workspace_mocker = example_workspace_2().unwrap();
    let workspace = ReleaseWorkspace::try_new(workspace_mocker.root()).unwrap();

    let crates = workspace.members().unwrap();
    ensure_release_order_consistency(crates).unwrap();

    let result = crates.iter().map(|crt| crt.name()).collect::<Vec<_>>();

    let expected_result = ["crate_b", "crate_c", "crate_a", "crate_d"]
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>();

    assert_eq!(expected_result, result);
}

#[test]
fn unreleasable_dependencies_error() {
    let workspace_mocker = example_workspace_3().unwrap();
    let workspace = ReleaseWorkspace::try_new(workspace_mocker.root()).unwrap();

    let err = workspace.release_selection().unwrap_err().to_string();

    assert!(err.contains("blocked"), "{}", err);
}

use CrateStateFlags::ChangedSincePreviousRelease;
use CrateStateFlags::DisallowedVersionReqViolated;
use CrateStateFlags::EnforcedVersionReqViolated;
use CrateStateFlags::IsWorkspaceDependency;
use CrateStateFlags::IsWorkspaceDevDependency;
use CrateStateFlags::Matched;
use CrateStateFlags::MissingChangelog;
use CrateStateFlags::MissingReadme;
use CrateStateFlags::UnreleasableViaChangelogFrontmatter;

#[test]
fn crate_state_block_consistency() {
    let flags: BitFlags<CrateStateFlags> = (&[
        Matched,
        DisallowedVersionReqViolated,
        UnreleasableViaChangelogFrontmatter,
    ] as &[CrateStateFlags])
        .iter()
        .cloned()
        .collect();

    let allowed_dev_dependency_blockers: BitFlags<CrateStateFlags> = (&[
        // CrateStateFlags::MissingChangelog
    ]
        as &[CrateStateFlags])
        .iter()
        .cloned()
        .collect();
    let state = CrateState::new(flags, allowed_dev_dependency_blockers, Default::default());

    assert!(
        !state.blocked_by().is_empty(),
        "should be blocked_by something. {:#?}",
        state
    );
    assert!(state.blocked(), "should be blocked. {:#?}", state);
    assert!(state.selected(), "should be selected. {:#?}", state);
    assert!(
        !state.release_selection(),
        "shouldn't be release selection{:#?}",
        state
    );
}

#[test]
fn crate_state_allowed_dev_dependency_blockers() {
    let flags: BitFlags<CrateStateFlags> = (&[
        IsWorkspaceDevDependency,
        UnreleasableViaChangelogFrontmatter,
        MissingChangelog,
        EnforcedVersionReqViolated,
    ] as &[CrateStateFlags])
        .iter()
        .cloned()
        .collect();

    let allowed_blockers: BitFlags<CrateStateFlags> = (&[
        UnreleasableViaChangelogFrontmatter,
        MissingChangelog,
        EnforcedVersionReqViolated,
    ] as &[CrateStateFlags])
        .iter()
        .cloned()
        .collect();

    let state = CrateState::new(flags, allowed_blockers, Default::default());

    assert!(
        state.blocked() && !state.blocked_by().is_empty() && state.disallowed_blockers().is_empty(),
        "blocked by: {:#?}, disallowed blockers: {:#?}",
        state.blocked_by(),
        state.disallowed_blockers(),
    );
}

#[test]
fn crate_state_allowed_selection_blockers() {
    let flags: BitFlags<CrateStateFlags> = (&[
        Matched,
        UnreleasableViaChangelogFrontmatter,
        MissingChangelog,
        EnforcedVersionReqViolated,
    ] as &[CrateStateFlags])
        .iter()
        .cloned()
        .collect();

    let allowed_blockers: BitFlags<CrateStateFlags> = (&[
        UnreleasableViaChangelogFrontmatter,
        MissingChangelog,
        EnforcedVersionReqViolated,
    ] as &[CrateStateFlags])
        .iter()
        .cloned()
        .collect();

    let state = CrateState::new(flags, Default::default(), allowed_blockers);

    assert!(
        state.blocked() && !state.blocked_by().is_empty() && state.disallowed_blockers().is_empty(),
        "blocked by: {:#?}, disallowed blockers: {:#?}",
        state.blocked_by(),
        state.disallowed_blockers(),
    );
}

// todo: add git tests here
// #[test]
// fn git_branch_management() -> {
//     let workspace_mocker = example_workspace_1().unwrap();
//     let workspace = ReleaseWorkspace::try_new(workspace_mocker.root()).unwrap();

// }
