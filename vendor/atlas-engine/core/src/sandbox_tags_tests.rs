// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use super::permission_profile_policy_tag;
use super::permission_profile_sandbox_tag;
use atlas_engine_protocol::config_types::WindowsSandboxLevel;
use atlas_engine_protocol::models::ManagedFileSystemPermissions;
use atlas_engine_protocol::models::PermissionProfile;
use atlas_engine_protocol::permissions::FileSystemAccessMode;
use atlas_engine_protocol::permissions::FileSystemPath;
use atlas_engine_protocol::permissions::FileSystemSandboxEntry;
use atlas_engine_protocol::permissions::FileSystemSandboxKind;
use atlas_engine_protocol::permissions::FileSystemSandboxPolicy;
use atlas_engine_protocol::permissions::NetworkSandboxPolicy;
use atlas_engine_sandboxing::SandboxType;
use atlas_engine_sandboxing::get_platform_sandbox;
use atlas_engine_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use std::path::Path;

#[test]
fn danger_full_access_is_untagged_even_when_linux_sandbox_defaults_apply() {
    let actual = permission_profile_sandbox_tag(
        &PermissionProfile::Disabled,
        WindowsSandboxLevel::Disabled,
        /*enforce_managed_network*/ false,
    );
    assert_eq!(actual, "none");
}

#[test]
fn external_sandbox_keeps_external_tag_when_linux_sandbox_defaults_apply() {
    let actual = permission_profile_sandbox_tag(
        &PermissionProfile::External {
            network: NetworkSandboxPolicy::Enabled,
        },
        WindowsSandboxLevel::Disabled,
        /*enforce_managed_network*/ false,
    );
    assert_eq!(actual, "external");
}

#[test]
fn default_linux_sandbox_uses_platform_sandbox_tag() {
    let actual = permission_profile_sandbox_tag(
        &PermissionProfile::read_only(),
        WindowsSandboxLevel::Disabled,
        /*enforce_managed_network*/ false,
    );
    let expected = get_platform_sandbox(/*windows_sandbox_enabled*/ false)
        .map(SandboxType::as_metric_tag)
        .unwrap_or("none");
    assert_eq!(actual, expected);
}

#[test]
fn profile_sandbox_tag_distinguishes_disabled_from_external() {
    assert_eq!(
        permission_profile_sandbox_tag(
            &PermissionProfile::Disabled,
            WindowsSandboxLevel::Disabled,
            /*enforce_managed_network*/ false,
        ),
        "none"
    );
    assert_eq!(
        permission_profile_sandbox_tag(
            &PermissionProfile::External {
                network: NetworkSandboxPolicy::Restricted,
            },
            WindowsSandboxLevel::Disabled,
            /*enforce_managed_network*/ false,
        ),
        "external"
    );
}

#[test]
fn unrestricted_managed_profile_with_enabled_network_is_untagged() {
    let profile = PermissionProfile::Managed {
        file_system: ManagedFileSystemPermissions::Unrestricted,
        network: NetworkSandboxPolicy::Enabled,
    };

    assert_eq!(
        permission_profile_sandbox_tag(
            &profile,
            WindowsSandboxLevel::Disabled,
            /*enforce_managed_network*/ false,
        ),
        "none"
    );
}

#[test]
fn root_write_managed_profile_with_enabled_network_is_untagged() {
    let profile = PermissionProfile::Managed {
        file_system: ManagedFileSystemPermissions::Restricted {
            entries: vec![FileSystemSandboxEntry {
                path: FileSystemPath::Special {
                    value: atlas_engine_protocol::permissions::FileSystemSpecialPath::Root,
                },
                access: FileSystemAccessMode::Write,
                missing_path_behavior: None,
            }],
            glob_scan_max_depth: None,
        },
        network: NetworkSandboxPolicy::Enabled,
    };

    assert_eq!(
        permission_profile_sandbox_tag(
            &profile,
            WindowsSandboxLevel::Disabled,
            /*enforce_managed_network*/ false,
        ),
        "none"
    );
}

#[test]
fn managed_network_enforcement_tags_unrestricted_profiles_as_sandboxed() {
    let profile = PermissionProfile::Managed {
        file_system: ManagedFileSystemPermissions::Unrestricted,
        network: NetworkSandboxPolicy::Enabled,
    };
    let expected = get_platform_sandbox(/*windows_sandbox_enabled*/ false)
        .map(SandboxType::as_metric_tag)
        .unwrap_or("none");

    assert_eq!(
        permission_profile_sandbox_tag(
            &profile,
            WindowsSandboxLevel::Disabled,
            /*enforce_managed_network*/ true,
        ),
        expected
    );
}

#[test]
fn profile_policy_tag_reports_closest_legacy_mode() {
    let cwd = AbsolutePathBuf::from_absolute_path(Path::new("/tmp/atlas-agent")).expect("absolute cwd");
    let writable_root = AbsolutePathBuf::from_absolute_path(Path::new("/tmp/atlas-agent/work"))
        .expect("absolute writable root");
    let profile = PermissionProfile::from_runtime_permissions(
        &FileSystemSandboxPolicy {
            kind: FileSystemSandboxKind::Restricted,
            glob_scan_max_depth: None,
            entries: vec![FileSystemSandboxEntry {
                path: FileSystemPath::Path {
                    path: writable_root,
                },
                access: FileSystemAccessMode::Write,
                missing_path_behavior: None,
            }],
        },
        NetworkSandboxPolicy::Restricted,
    );

    assert_eq!(
        permission_profile_policy_tag(&profile, cwd.as_path()),
        "workspace-write"
    );
}
