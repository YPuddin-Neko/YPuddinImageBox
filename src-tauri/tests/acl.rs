use std::collections::BTreeSet;

use tauri::utils::{
    acl::{get_capabilities, resolved::Resolved, ExecutionContext},
    platform::Target,
};

fn registered_commands() -> BTreeSet<&'static str> {
    let source = include_str!("../src/lib.rs");
    let (_, handler) = source
        .split_once(".invoke_handler(tauri::generate_handler![")
        .expect("the application command handler must be present");
    let (handler, _) = handler.split_once(']').expect("the command handler must be closed");
    let mut commands = BTreeSet::new();
    for entry in handler.split(',').map(str::trim).filter(|entry| !entry.is_empty()) {
        let command = entry.strip_prefix("commands::").expect("expected an application command");
        assert!(command.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "unexpected command entry: {entry}");
        assert!(commands.insert(command), "duplicate command registration: {command}");
    }
    assert!(!commands.is_empty());
    commands
}

fn resolved_acl(target: Target) -> Resolved {
    let manifests = serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/acl-manifests.json"))).unwrap();
    let capabilities = serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/capabilities.json"))).unwrap();
    let config = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let capabilities = get_capabilities(&config, capabilities, None).unwrap();
    Resolved::resolve(&manifests, capabilities, target).unwrap()
}

#[test]
fn all_registered_commands_are_allowed_only_in_the_local_main_window() {
    let registered = registered_commands();
    for target in [Target::MacOS, Target::Windows, Target::Linux] {
        let resolved = resolved_acl(target);
        assert!(resolved.has_app_acl, "application ACL must be enabled on {target}");
        let allowed: BTreeSet<_> = resolved.allowed_commands.keys()
            .map(String::as_str)
            .filter(|command| !command.starts_with("plugin:"))
            .collect();
        assert_eq!(allowed, registered, "registered commands and generated application ACL differ on {target}");
        for command in &registered {
            assert!(!resolved.denied_commands.contains_key(*command), "{command} is explicitly denied on {target}");
            for permission in &resolved.allowed_commands[*command] {
                assert_eq!(permission.context, ExecutionContext::Local, "{command} must not be accessible remotely on {target}");
                assert_eq!(permission.windows.iter().map(|pattern| pattern.as_str()).collect::<Vec<_>>(), ["main"], "{command} must be limited to the main window on {target}");
                assert!(permission.webviews.is_empty(), "{command} must not grant access to additional webviews on {target}");
            }
        }
    }
}

#[test]
fn remote_windows_can_only_submit_x_bridge_responses() {
    for target in [Target::MacOS, Target::Windows, Target::Linux] {
        let resolved = resolved_acl(target);
        let mut remote_commands = BTreeSet::new();
        for (command, permissions) in &resolved.allowed_commands {
            for permission in permissions {
                if matches!(permission.context, ExecutionContext::Remote { .. }) {
                    remote_commands.insert(command.as_str());
                    assert_eq!(permission.windows.iter().map(|pattern| pattern.as_str()).collect::<Vec<_>>(), ["x-capture"], "remote access must be limited to X capture on {target}");
                    assert!(permission.webviews.is_empty());
                }
            }
        }
        assert_eq!(remote_commands, BTreeSet::from(["plugin:x|bridge_response"]), "unexpected remote command access on {target}");
    }
}
