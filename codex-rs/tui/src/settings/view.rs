use anyhow::Context;
use anyhow::Result;
use codex_core::config::Config;
use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::custom_prompt_view::CustomPromptView;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;

use super::data::SettingItemData;
use super::data::SettingsRootItemData;
use super::data::SettingsRootItemKind;
use super::data::SettingsScope;
use super::data::SettingsScreen;
use super::data::build_setting_items_with_features;
use super::data::build_settings_root_items_with_features;
use super::data::build_settings_section_view_data_with_features;
use super::data::parse_scalar_input;
use super::data::parse_toml_fragment;
use super::schema::SchemaNodeKind;
use super::schema::load_settings_schema;

pub(crate) const SETTINGS_ROOT_VIEW_ID: &str = "settings.root";
pub(crate) const SETTINGS_SECTION_VIEW_ID: &str = "settings.section";
pub(crate) const SETTINGS_SCOPE_VIEW_ID: &str = "settings.scope";
const CLEAR_SENTINEL: &str = "__clear__";

pub(crate) fn build_settings_view_params(
    config: &Config,
    scope: SettingsScope,
    screen: &SettingsScreen,
    selected_item_key: Option<&str>,
) -> Result<SelectionViewParams> {
    let scope = scope.normalized(config.active_profile.as_deref());
    let schema = load_settings_schema().context("load settings schema")?;
    let effective_config = config.config_layer_stack.effective_config();
    let origins = config.config_layer_stack.origins();
    let mut items = vec![scope_selection_item(
        scope,
        screen.clone(),
        config.active_profile.as_deref(),
    )];
    let initial_selected_idx = match screen {
        SettingsScreen::Root => {
            let root_items = build_settings_root_items_with_features(
                &schema,
                &effective_config,
                &origins,
                Some(&config.features),
                config.active_profile.as_deref(),
                scope,
            );
            let content_item_keys = root_items
                .iter()
                .map(|item| item.item_key.as_str())
                .collect::<Vec<_>>();
            items.extend(
                root_items
                    .iter()
                    .map(|item| root_selection_item(item, scope)),
            );
            selected_item_key.and_then(|selected_item_key| {
                content_item_keys
                    .iter()
                    .position(|item_key| *item_key == selected_item_key)
                    .map(|idx| idx + 1)
            })
        }
        SettingsScreen::Section { section_key } => {
            let section_view = build_settings_section_view_data_with_features(
                &schema,
                &effective_config,
                &origins,
                Some(&config.features),
                config.active_profile.as_deref(),
                scope,
                section_key,
            );
            let content_item_keys = section_view
                .section_item
                .iter()
                .chain(section_view.items.iter())
                .map(|item| item.item_key.as_str())
                .collect::<Vec<_>>();
            if let Some(section_item) = section_view.section_item.as_ref() {
                items.push(setting_selection_item(
                    &section_item.setting,
                    scope,
                    screen.clone(),
                ));
            }
            items.extend(
                section_view
                    .items
                    .iter()
                    .map(|item| setting_selection_item(&item.setting, scope, screen.clone())),
            );
            if section_view.section_item.is_none() && section_view.items.is_empty() {
                items.push(empty_section_selection_item(section_key));
            }
            selected_item_key.and_then(|selected_item_key| {
                content_item_keys
                    .iter()
                    .position(|item_key| *item_key == selected_item_key)
                    .map(|idx| idx + 1)
            })
        }
    };

    Ok(SelectionViewParams {
        view_id: Some(settings_view_id(screen)),
        title: Some(settings_title(screen)),
        subtitle: Some(settings_subtitle(
            screen,
            scope,
            config.active_profile.as_deref(),
        )),
        footer_note: Some(
            Line::from(format!(
                "Type {CLEAR_SENTINEL} in an editor to clear a saved override."
            ))
            .dim(),
        ),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        is_searchable: true,
        search_placeholder: Some("Type to filter settings".to_string()),
        initial_selected_idx,
        ..Default::default()
    })
}

pub(crate) fn build_settings_scope_picker_params(
    current_scope: SettingsScope,
    current_screen: SettingsScreen,
    active_profile: Option<&str>,
) -> SelectionViewParams {
    let current_scope = current_scope.normalized(active_profile);
    let global_screen = current_screen.clone();
    let global_actions: Vec<SelectionAction> = vec![Box::new(move |tx: &AppEventSender| {
        tx.send(AppEvent::OpenSettings {
            scope: SettingsScope::Global,
            screen: global_screen.clone(),
            selected_item_key: None,
        });
    })];

    let mut items = vec![SelectionItem {
        name: "User config".to_string(),
        description: Some("Write to your top-level config.toml.".to_string()),
        selected_description: Some(
            "Writes the selected key to your user config.toml and applies everywhere unless a profile override wins."
                .to_string(),
        ),
        is_current: current_scope == SettingsScope::Global,
        actions: global_actions,
        dismiss_on_select: true,
        search_value: Some("user config global".to_string()),
        ..Default::default()
    }];

    let profile_name = active_profile.map(ToOwned::to_owned);
    let profile_screen = current_screen;
    let profile_actions: Vec<SelectionAction> = vec![Box::new(move |tx: &AppEventSender| {
        tx.send(AppEvent::OpenSettings {
            scope: SettingsScope::ActiveProfile,
            screen: profile_screen.clone(),
            selected_item_key: None,
        });
    })];
    items.push(SelectionItem {
        name: "Active profile".to_string(),
        description: Some(match active_profile {
            Some(profile) => format!("Write under [profiles.{profile}]."),
            None => "No active profile is selected.".to_string(),
        }),
        selected_description: profile_name.as_ref().map(|profile| {
            format!(
                "Writes the selected key under [profiles.{profile}] so only that profile changes."
            )
        }),
        is_current: current_scope == SettingsScope::ActiveProfile,
        is_disabled: active_profile.is_none(),
        actions: profile_actions,
        dismiss_on_select: true,
        search_value: profile_name.map(|profile| format!("profile {profile}")),
        disabled_reason: active_profile
            .is_none()
            .then_some("No active profile is available.".to_string()),
        ..Default::default()
    });

    SelectionViewParams {
        view_id: Some(SETTINGS_SCOPE_VIEW_ID),
        title: Some("Write scope".to_string()),
        subtitle: Some("Choose where /settings saves changes.".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
}

pub(crate) fn build_setting_editor_view(
    config: &Config,
    key_path: &str,
    scope: SettingsScope,
    screen: SettingsScreen,
    app_event_tx: AppEventSender,
) -> Result<CustomPromptView> {
    let scope = scope.normalized(config.active_profile.as_deref());
    let schema = load_settings_schema().context("load settings schema")?;
    let effective_config = config.config_layer_stack.effective_config();
    let origins = config.config_layer_stack.origins();
    let item = build_setting_items_with_features(
        &schema,
        &effective_config,
        &origins,
        Some(&config.features),
        config.active_profile.as_deref(),
        scope,
    )
    .into_iter()
    .find(|item| item.node.key_path == key_path)
    .with_context(|| format!("setting `{key_path}` not found"))?;

    let kind = item.node.kind;
    let key_path = item.node.key_path.clone();
    let submit_tx = app_event_tx;
    let title = format!("Edit {key_path}");
    let placeholder = editor_placeholder(&item);
    let context_label = Some(editor_context_label(
        &item,
        scope,
        config.active_profile.as_deref(),
    ));

    Ok(CustomPromptView::new(
        title,
        placeholder,
        context_label,
        Box::new(move |input: String| {
            let trimmed = input.trim();
            if trimmed == CLEAR_SENTINEL {
                submit_tx.send(AppEvent::SaveSettingValue {
                    key_path: key_path.clone(),
                    scope,
                    screen: screen.clone(),
                    value: None,
                });
                return Ok(());
            }

            let value = parse_editor_input(kind, &input)?;
            submit_tx.send(AppEvent::SaveSettingValue {
                key_path: key_path.clone(),
                scope,
                screen: screen.clone(),
                value: Some(value),
            });
            Ok(())
        }),
    )
    .with_initial_text(item.editor_value))
}

fn settings_view_id(screen: &SettingsScreen) -> &'static str {
    match screen {
        SettingsScreen::Root => SETTINGS_ROOT_VIEW_ID,
        SettingsScreen::Section { .. } => SETTINGS_SECTION_VIEW_ID,
    }
}

fn settings_title(screen: &SettingsScreen) -> String {
    match screen {
        SettingsScreen::Root => "Settings".to_string(),
        SettingsScreen::Section { section_key } => format!("Settings / {section_key}"),
    }
}

fn settings_subtitle(
    screen: &SettingsScreen,
    scope: SettingsScope,
    active_profile: Option<&str>,
) -> String {
    match screen {
        SettingsScreen::Root => match scope {
            SettingsScope::Global => {
                "Browse settings sections and edit your user config.toml.".to_string()
            }
            SettingsScope::ActiveProfile => match active_profile {
                Some(profile) => {
                    format!("Browse settings sections for the active profile `{profile}`.")
                }
                None => "Browse settings sections and edit your user config.toml.".to_string(),
            },
        },
        SettingsScreen::Section { section_key } => match scope {
            SettingsScope::Global => {
                format!("Browse and edit settings under `{section_key}` in user config.toml.")
            }
            SettingsScope::ActiveProfile => match active_profile {
                Some(profile) => {
                    format!("Browse and edit `{section_key}` for the active profile `{profile}`.")
                }
                None => {
                    format!("Browse and edit settings under `{section_key}` in user config.toml.")
                }
            },
        },
    }
}

fn scope_selection_item(
    scope: SettingsScope,
    screen: SettingsScreen,
    active_profile: Option<&str>,
) -> SelectionItem {
    SelectionItem {
        name: "Write scope".to_string(),
        description: Some(match scope {
            SettingsScope::Global => "Currently writing to user config.toml.".to_string(),
            SettingsScope::ActiveProfile => match active_profile {
                Some(profile) => format!("Currently writing to [profiles.{profile}]."),
                None => "No active profile is available; using user config.toml.".to_string(),
            },
        }),
        selected_description: Some(
            "Switch between user config.toml and the active profile without leaving /settings."
                .to_string(),
        ),
        actions: vec![Box::new(move |tx: &AppEventSender| {
            tx.send(AppEvent::OpenSettingsScopePicker {
                current_scope: scope,
                current_screen: screen.clone(),
            });
        })],
        dismiss_on_select: false,
        search_value: Some("write scope global profile".to_string()),
        ..Default::default()
    }
}

fn empty_section_selection_item(section_key: &str) -> SelectionItem {
    SelectionItem {
        name: "No settings available".to_string(),
        description: Some(format!(
            "No configurable keys under `{section_key}` are available in this scope."
        )),
        is_disabled: true,
        disabled_reason: Some("Try another scope or go back.".to_string()),
        search_value: Some(format!("empty {section_key}")),
        ..Default::default()
    }
}

fn root_selection_item(item: &SettingsRootItemData, scope: SettingsScope) -> SelectionItem {
    let actions: Vec<SelectionAction> = if item.disabled_reason.is_some() {
        Vec::new()
    } else {
        match &item.kind {
            SettingsRootItemKind::Section { section_key } => {
                let section_key = section_key.clone();
                vec![Box::new(move |tx: &AppEventSender| {
                    tx.send(AppEvent::OpenSettings {
                        scope,
                        screen: SettingsScreen::Section {
                            section_key: section_key.clone(),
                        },
                        selected_item_key: None,
                    });
                }) as SelectionAction]
            }
            SettingsRootItemKind::Setting(setting) => {
                let key_path = setting.node.key_path.clone();
                vec![Box::new(move |tx: &AppEventSender| {
                    tx.send(AppEvent::OpenSettingEditor {
                        key_path: key_path.clone(),
                        scope,
                        screen: SettingsScreen::Root,
                    });
                })]
            }
        }
    };

    SelectionItem {
        name: item.label.clone(),
        description: item.description.clone(),
        selected_description: item.selected_description.clone(),
        actions,
        dismiss_on_select: false,
        search_value: Some(item.search_value.clone()),
        disabled_reason: item.disabled_reason.clone(),
        ..Default::default()
    }
}

fn setting_selection_item(
    item: &SettingItemData,
    scope: SettingsScope,
    screen: SettingsScreen,
) -> SelectionItem {
    let key_path = item.node.key_path.clone();
    let actions: Vec<SelectionAction> = if item.disabled_reason.is_some() {
        Vec::new()
    } else {
        vec![Box::new(move |tx: &AppEventSender| {
            tx.send(AppEvent::OpenSettingEditor {
                key_path: key_path.clone(),
                scope,
                screen: screen.clone(),
            });
        })]
    };

    SelectionItem {
        name: item.label.clone(),
        description: item.description.clone(),
        selected_description: item.selected_description.clone(),
        actions,
        dismiss_on_select: false,
        search_value: Some(item.search_value.clone()),
        disabled_reason: item.disabled_reason.clone(),
        ..Default::default()
    }
}

fn editor_context_label(
    item: &SettingItemData,
    scope: SettingsScope,
    active_profile: Option<&str>,
) -> String {
    let scope_label = match scope {
        SettingsScope::Global => "Scope: user config".to_string(),
        SettingsScope::ActiveProfile => match active_profile {
            Some(profile) => format!("Scope: profile `{profile}`"),
            None => "Scope: user config".to_string(),
        },
    };
    let source_label = item
        .category_tag
        .as_deref()
        .map(|source| format!("Source: {source}"))
        .unwrap_or_else(|| "Source: default".to_string());
    format!("{scope_label} | {source_label}")
}

fn editor_placeholder(item: &SettingItemData) -> String {
    let mut parts = Vec::new();
    match item.node.kind {
        SchemaNodeKind::Boolean => parts.push("Enter true or false.".to_string()),
        SchemaNodeKind::Integer => parts.push("Enter an integer.".to_string()),
        SchemaNodeKind::Number => parts.push("Enter a number.".to_string()),
        SchemaNodeKind::String => parts.push("Enter a string value.".to_string()),
        SchemaNodeKind::Array | SchemaNodeKind::Object | SchemaNodeKind::Unknown => {
            parts.push("Enter a TOML value or table fragment.".to_string())
        }
    }
    if !item.node.enum_values.is_empty() {
        parts.push(format!("Options: {}.", item.node.enum_values.join(", ")));
    }
    parts.push(format!("Type {CLEAR_SENTINEL} to clear this override."));
    parts.join(" ")
}

fn parse_editor_input(kind: SchemaNodeKind, input: &str) -> Result<toml::Value, String> {
    match kind {
        SchemaNodeKind::Boolean
        | SchemaNodeKind::Integer
        | SchemaNodeKind::Number
        | SchemaNodeKind::String => parse_scalar_input(kind, input),
        SchemaNodeKind::Array | SchemaNodeKind::Object | SchemaNodeKind::Unknown => {
            parse_toml_fragment(input)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::data::SettingsScope;
    use crate::settings::data::SettingsScreen;
    use codex_core::config::Config;
    use codex_core::config::ConfigBuilder;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::build_settings_scope_picker_params;
    use super::build_settings_view_params;

    fn scope_picker_summary(params: &crate::bottom_pane::SelectionViewParams) -> String {
        let mut lines = vec![
            format!("title: {}", params.title.as_deref().unwrap_or_default()),
            format!(
                "subtitle: {}",
                params.subtitle.as_deref().unwrap_or_default()
            ),
        ];
        for item in &params.items {
            lines.push(format!("item: {}", item.name));
            lines.push(format!(
                "  description: {}",
                item.description.as_deref().unwrap_or_default()
            ));
            lines.push(format!("  current: {}", item.is_current));
            lines.push(format!("  disabled: {}", item.is_disabled));
            lines.push(format!(
                "  disabled_reason: {}",
                item.disabled_reason.as_deref().unwrap_or_default()
            ));
        }
        lines.join("\n")
    }

    fn settings_view_focus_summary(params: &crate::bottom_pane::SelectionViewParams) -> String {
        const FOCUS_KEYS: [&str; 6] = [
            "Write scope",
            "model",
            "audio",
            "audio.microphone",
            "service_tier",
            "include_apply_patch_tool",
        ];
        let mut lines = vec![
            format!("title: {}", params.title.as_deref().unwrap_or_default()),
            format!(
                "subtitle: {}",
                params.subtitle.as_deref().unwrap_or_default()
            ),
        ];
        for key in FOCUS_KEYS {
            let matches = params.items.iter().filter(|item| item.name == key).count();
            lines.push(format!("{key}: {matches}"));
        }
        lines.join("\n")
    }

    fn section_view_focus_summary(params: &crate::bottom_pane::SelectionViewParams) -> String {
        const FOCUS_KEYS: [&str; 4] = ["Write scope", "Edit this section", "microphone", "speaker"];
        let mut lines = vec![
            format!("title: {}", params.title.as_deref().unwrap_or_default()),
            format!(
                "subtitle: {}",
                params.subtitle.as_deref().unwrap_or_default()
            ),
        ];
        for key in FOCUS_KEYS {
            let matches = params.items.iter().filter(|item| item.name == key).count();
            lines.push(format!("{key}: {matches}"));
        }
        lines.join("\n")
    }

    fn model_section_focus_summary(params: &crate::bottom_pane::SelectionViewParams) -> String {
        const FOCUS_KEYS: [&str; 7] = [
            "Write scope",
            "model",
            "plan_mode_reasoning_effort",
            "provider",
            "reasoning_effort",
            "review_model",
            "service_tier",
        ];
        let mut lines = vec![
            format!("title: {}", params.title.as_deref().unwrap_or_default()),
            format!(
                "subtitle: {}",
                params.subtitle.as_deref().unwrap_or_default()
            ),
        ];
        for key in FOCUS_KEYS {
            let matches = params.items.iter().filter(|item| item.name == key).count();
            lines.push(format!("{key}: {matches}"));
        }
        lines.join("\n")
    }

    fn grouped_root_focus_summary(params: &crate::bottom_pane::SelectionViewParams) -> String {
        const FOCUS_KEYS: [&str; 26] = [
            "auth",
            "extensions",
            "features",
            "mcp",
            "model",
            "notifications",
            "permissions",
            "profile",
            "project",
            "prompting",
            "reasoning_output",
            "storage",
            "telemetry",
            "tools",
            "tui",
            "voice",
            "windows",
            "agents",
            "memories",
            "audio",
            "check_for_update_on_startup",
            "commit_attribution",
            "disable_paste_burst",
            "instructions",
            "notify",
            "windows_wsl_setup_acknowledged",
        ];
        let mut lines = vec![
            format!("title: {}", params.title.as_deref().unwrap_or_default()),
            format!(
                "subtitle: {}",
                params.subtitle.as_deref().unwrap_or_default()
            ),
        ];
        for key in FOCUS_KEYS {
            let matches = params.items.iter().filter(|item| item.name == key).count();
            lines.push(format!("{key}: {matches}"));
        }
        lines.join("\n")
    }

    fn features_section_focus_summary(params: &crate::bottom_pane::SelectionViewParams) -> String {
        const FOCUS_KEYS: [&str; 5] = [
            "Write scope",
            "JavaScript REPL",
            "Smart Approvals",
            "default_mode_request_user_input",
            "suppress_unstable_features_warning",
        ];
        let mut lines = vec![
            format!("title: {}", params.title.as_deref().unwrap_or_default()),
            format!(
                "subtitle: {}",
                params.subtitle.as_deref().unwrap_or_default()
            ),
        ];
        for key in FOCUS_KEYS {
            let matches = params.items.iter().filter(|item| item.name == key).count();
            lines.push(format!("{key}: {matches}"));
        }
        lines.join("\n")
    }

    fn telemetry_section_focus_summary(params: &crate::bottom_pane::SelectionViewParams) -> String {
        const FOCUS_KEYS: [&str; 7] = [
            "Write scope",
            "analytics",
            "analytics.enabled",
            "feedback",
            "feedback.enabled",
            "otel",
            "otel.environment",
        ];
        let mut lines = vec![
            format!("title: {}", params.title.as_deref().unwrap_or_default()),
            format!(
                "subtitle: {}",
                params.subtitle.as_deref().unwrap_or_default()
            ),
        ];
        for key in FOCUS_KEYS {
            let matches = params.items.iter().filter(|item| item.name == key).count();
            lines.push(format!("{key}: {matches}"));
        }
        lines.join("\n")
    }

    fn notifications_section_focus_summary(
        params: &crate::bottom_pane::SelectionViewParams,
    ) -> String {
        const FOCUS_KEYS: [&str; 6] = [
            "Write scope",
            "check_for_update_on_startup",
            "external_command",
            "hide_full_access_warning",
            "notification_method",
            "notifications",
        ];
        let mut lines = vec![
            format!("title: {}", params.title.as_deref().unwrap_or_default()),
            format!(
                "subtitle: {}",
                params.subtitle.as_deref().unwrap_or_default()
            ),
        ];
        for key in FOCUS_KEYS {
            let matches = params.items.iter().filter(|item| item.name == key).count();
            lines.push(format!("{key}: {matches}"));
        }
        lines.join("\n")
    }

    fn auth_section_focus_summary(params: &crate::bottom_pane::SelectionViewParams) -> String {
        const FOCUS_KEYS: [&str; 4] = [
            "Write scope",
            "chatgpt_workspace_id",
            "credentials_store",
            "login_method",
        ];
        let mut lines = vec![
            format!("title: {}", params.title.as_deref().unwrap_or_default()),
            format!(
                "subtitle: {}",
                params.subtitle.as_deref().unwrap_or_default()
            ),
        ];
        for key in FOCUS_KEYS {
            let matches = params.items.iter().filter(|item| item.name == key).count();
            lines.push(format!("{key}: {matches}"));
        }
        lines.join("\n")
    }

    fn tui_section_focus_summary(params: &crate::bottom_pane::SelectionViewParams) -> String {
        const FOCUS_KEYS: [&str; 6] = [
            "Write scope",
            "Edit this section",
            "disable_paste_burst",
            "file_opener",
            "theme",
            "notification_method",
        ];
        let mut lines = vec![
            format!("title: {}", params.title.as_deref().unwrap_or_default()),
            format!(
                "subtitle: {}",
                params.subtitle.as_deref().unwrap_or_default()
            ),
        ];
        for key in FOCUS_KEYS {
            let matches = params.items.iter().filter(|item| item.name == key).count();
            lines.push(format!("{key}: {matches}"));
        }
        lines.join("\n")
    }

    fn tools_section_focus_summary(params: &crate::bottom_pane::SelectionViewParams) -> String {
        const FOCUS_KEYS: [&str; 9] = [
            "Write scope",
            "Edit this section",
            "js_repl_node_module_dirs",
            "js_repl_node_path",
            "tools_view_image",
            "view_image",
            "web_search",
            "web_search_mode",
            "zsh_path",
        ];
        let mut lines = vec![
            format!("title: {}", params.title.as_deref().unwrap_or_default()),
            format!(
                "subtitle: {}",
                params.subtitle.as_deref().unwrap_or_default()
            ),
        ];
        for key in FOCUS_KEYS {
            let matches = params.items.iter().filter(|item| item.name == key).count();
            lines.push(format!("{key}: {matches}"));
        }
        lines.join("\n")
    }

    fn voice_section_focus_summary(params: &crate::bottom_pane::SelectionViewParams) -> String {
        const FOCUS_KEYS: [&str; 6] = [
            "Write scope",
            "audio",
            "microphone",
            "realtime",
            "realtime_ws_model",
            "version",
        ];
        let mut lines = vec![
            format!("title: {}", params.title.as_deref().unwrap_or_default()),
            format!(
                "subtitle: {}",
                params.subtitle.as_deref().unwrap_or_default()
            ),
        ];
        for key in FOCUS_KEYS {
            let matches = params.items.iter().filter(|item| item.name == key).count();
            lines.push(format!("{key}: {matches}"));
        }
        lines.join("\n")
    }

    fn tools_global_section_focus_summary(
        params: &crate::bottom_pane::SelectionViewParams,
    ) -> String {
        const FOCUS_KEYS: [&str; 4] = [
            "Write scope",
            "Edit this section",
            "background_terminal_max_timeout",
            "tool_output_token_limit",
        ];
        let mut lines = vec![
            format!("title: {}", params.title.as_deref().unwrap_or_default()),
            format!(
                "subtitle: {}",
                params.subtitle.as_deref().unwrap_or_default()
            ),
        ];
        for key in FOCUS_KEYS {
            let matches = params.items.iter().filter(|item| item.name == key).count();
            lines.push(format!("{key}: {matches}"));
        }
        lines.join("\n")
    }

    async fn settings_test_config(active_profile: Option<&str>) -> Config {
        let codex_home = std::env::temp_dir();
        let mut config = ConfigBuilder::default()
            .codex_home(codex_home)
            .build()
            .await
            .expect("config");
        let temp = tempdir().expect("tempdir");
        let config_toml_path =
            AbsolutePathBuf::try_from(temp.path().join("config.toml")).expect("absolute path");
        let user_config = toml::from_str(
            r#"
model = "gpt-5"
[audio]
microphone = "Desk Mic"
[profiles.dev]
model = "o3"
include_apply_patch_tool = true
"#,
        )
        .expect("config");
        config.config_layer_stack = config
            .config_layer_stack
            .with_user_config(&config_toml_path, user_config);
        config.active_profile = active_profile.map(ToOwned::to_owned);
        config
    }

    #[test]
    fn settings_scope_picker_disables_missing_profile() {
        let params =
            build_settings_scope_picker_params(SettingsScope::Global, SettingsScreen::Root, None);
        assert_eq!(params.items.len(), 2);
        assert!(params.items[1].is_disabled);
    }

    #[test]
    fn settings_scope_picker_snapshot() {
        let params = build_settings_scope_picker_params(
            SettingsScope::ActiveProfile,
            SettingsScreen::Root,
            Some("project"),
        );
        assert_snapshot!("settings_scope_picker", scope_picker_summary(&params));
    }

    #[tokio::test]
    async fn settings_view_global_snapshot() {
        let config = settings_test_config(None).await;
        let params =
            build_settings_view_params(&config, SettingsScope::Global, &SettingsScreen::Root, None)
                .expect("settings view");

        assert_snapshot!(
                                                                                            settings_view_focus_summary(&params),
                                                                                            @r"
title: Settings
subtitle: Browse settings sections and edit your user config.toml.
Write scope: 1
model: 1
audio: 0
audio.microphone: 0
service_tier: 0
include_apply_patch_tool: 0
"
                                                                                        );
    }

    #[tokio::test]
    async fn settings_view_global_manual_grouping_snapshot() {
        let config = settings_test_config(None).await;
        let params =
            build_settings_view_params(&config, SettingsScope::Global, &SettingsScreen::Root, None)
                .expect("settings view");

        assert_snapshot!(
                                                            grouped_root_focus_summary(&params),
                                                            @r"
title: Settings
subtitle: Browse settings sections and edit your user config.toml.
auth: 1
extensions: 1
features: 1
mcp: 1
model: 1
notifications: 1
permissions: 1
profile: 1
project: 1
prompting: 1
reasoning_output: 1
storage: 1
telemetry: 1
tools: 1
tui: 1
voice: 1
windows: 1
agents: 1
memories: 1
audio: 0
check_for_update_on_startup: 0
commit_attribution: 0
disable_paste_burst: 0
instructions: 0
notify: 0
windows_wsl_setup_acknowledged: 0
"
                                                        );
    }

    #[tokio::test]
    async fn settings_features_section_snapshot() {
        let config = settings_test_config(None).await;
        let params = build_settings_view_params(
            &config,
            SettingsScope::Global,
            &SettingsScreen::Section {
                section_key: "features".to_string(),
            },
            None,
        )
        .expect("features settings section");

        assert_snapshot!(
                                                    features_section_focus_summary(&params),
                                                    @r"
title: Settings / features
subtitle: Browse and edit settings under `features` in user config.toml.
Write scope: 1
JavaScript REPL: 1
Smart Approvals: 1
default_mode_request_user_input: 1
suppress_unstable_features_warning: 1
"
                                                );
    }

    #[tokio::test]
    async fn settings_view_profile_snapshot() {
        let config = settings_test_config(Some("dev")).await;
        let params = build_settings_view_params(
            &config,
            SettingsScope::ActiveProfile,
            &SettingsScreen::Root,
            None,
        )
        .expect("settings view");

        assert_snapshot!(
                                                                                            settings_view_focus_summary(&params),
                                                                                            @r"
title: Settings
subtitle: Browse settings sections for the active profile `dev`.
Write scope: 1
model: 1
audio: 0
audio.microphone: 0
service_tier: 0
include_apply_patch_tool: 0
"
                                                                                        );
    }

    #[tokio::test]
    async fn settings_section_snapshot() {
        let config = settings_test_config(None).await;
        let params = build_settings_view_params(
            &config,
            SettingsScope::Global,
            &SettingsScreen::Section {
                section_key: "audio".to_string(),
            },
            None,
        )
        .expect("settings section");

        assert_snapshot!(
                                                                                            section_view_focus_summary(&params),
                                                                                            @r"
title: Settings / audio
subtitle: Browse and edit settings under `audio` in user config.toml.
Write scope: 1
Edit this section: 1
microphone: 1
speaker: 1
"
                                                                                        );
    }

    #[tokio::test]
    async fn settings_model_section_snapshot() {
        let config = settings_test_config(None).await;
        let params = build_settings_view_params(
            &config,
            SettingsScope::Global,
            &SettingsScreen::Section {
                section_key: "model".to_string(),
            },
            None,
        )
        .expect("model settings section");

        assert_snapshot!(
                                                            model_section_focus_summary(&params),
                                                            @r"
title: Settings / model
subtitle: Browse and edit settings under `model` in user config.toml.
Write scope: 1
model: 1
plan_mode_reasoning_effort: 1
provider: 1
reasoning_effort: 1
review_model: 1
service_tier: 1
"
                                                        );
    }

    #[tokio::test]
    async fn settings_auth_section_snapshot() {
        let config = settings_test_config(None).await;
        let params = build_settings_view_params(
            &config,
            SettingsScope::Global,
            &SettingsScreen::Section {
                section_key: "auth".to_string(),
            },
            None,
        )
        .expect("auth settings section");

        assert_snapshot!(
                                                            auth_section_focus_summary(&params),
                                                            @r"
title: Settings / auth
subtitle: Browse and edit settings under `auth` in user config.toml.
Write scope: 1
chatgpt_workspace_id: 1
credentials_store: 1
login_method: 1
"
                                                        );
    }

    #[tokio::test]
    async fn settings_notifications_section_snapshot() {
        let config = settings_test_config(None).await;
        let params = build_settings_view_params(
            &config,
            SettingsScope::Global,
            &SettingsScreen::Section {
                section_key: "notifications".to_string(),
            },
            None,
        )
        .expect("notifications settings section");

        assert_snapshot!(
                                                    notifications_section_focus_summary(&params),
                                                    @r"
title: Settings / notifications
subtitle: Browse and edit settings under `notifications` in user config.toml.
Write scope: 1
check_for_update_on_startup: 1
external_command: 1
hide_full_access_warning: 1
notification_method: 1
notifications: 1
"
                                                );
    }

    #[tokio::test]
    async fn settings_telemetry_section_snapshot() {
        let config = settings_test_config(None).await;
        let params = build_settings_view_params(
            &config,
            SettingsScope::Global,
            &SettingsScreen::Section {
                section_key: "telemetry".to_string(),
            },
            None,
        )
        .expect("telemetry settings section");

        assert_snapshot!(
                                                    telemetry_section_focus_summary(&params),
                                                    @r"
title: Settings / telemetry
subtitle: Browse and edit settings under `telemetry` in user config.toml.
Write scope: 1
analytics: 1
analytics.enabled: 1
feedback: 1
feedback.enabled: 1
otel: 1
otel.environment: 1
"
                                                );
    }

    #[tokio::test]
    async fn settings_tui_section_snapshot() {
        let config = settings_test_config(None).await;
        let params = build_settings_view_params(
            &config,
            SettingsScope::Global,
            &SettingsScreen::Section {
                section_key: "tui".to_string(),
            },
            None,
        )
        .expect("tui settings section");

        assert_snapshot!(
                                                    tui_section_focus_summary(&params),
                                                    @r"
title: Settings / tui
subtitle: Browse and edit settings under `tui` in user config.toml.
Write scope: 1
Edit this section: 1
disable_paste_burst: 1
file_opener: 1
theme: 1
notification_method: 0
"
                                                );
    }

    #[tokio::test]
    async fn settings_tools_section_snapshot() {
        let config = settings_test_config(Some("dev")).await;
        let params = build_settings_view_params(
            &config,
            SettingsScope::ActiveProfile,
            &SettingsScreen::Section {
                section_key: "tools".to_string(),
            },
            None,
        )
        .expect("tools settings section");

        assert_snapshot!(
                                                            tools_section_focus_summary(&params),
                                                            @r"
title: Settings / tools
subtitle: Browse and edit `tools` for the active profile `dev`.
Write scope: 1
Edit this section: 1
js_repl_node_module_dirs: 1
js_repl_node_path: 1
tools_view_image: 1
view_image: 1
web_search: 1
web_search_mode: 1
zsh_path: 1
"
                                                        );
    }

    #[tokio::test]
    async fn settings_voice_section_snapshot() {
        let config = settings_test_config(None).await;
        let params = build_settings_view_params(
            &config,
            SettingsScope::Global,
            &SettingsScreen::Section {
                section_key: "voice".to_string(),
            },
            None,
        )
        .expect("voice settings section");

        assert_snapshot!(
                                                    voice_section_focus_summary(&params),
                                                    @r"
title: Settings / voice
subtitle: Browse and edit settings under `voice` in user config.toml.
Write scope: 1
audio: 1
microphone: 1
realtime: 1
realtime_ws_model: 1
version: 1
"
                                                );
    }

    #[tokio::test]
    async fn settings_tools_global_section_snapshot() {
        let config = settings_test_config(None).await;
        let params = build_settings_view_params(
            &config,
            SettingsScope::Global,
            &SettingsScreen::Section {
                section_key: "tools".to_string(),
            },
            None,
        )
        .expect("global tools settings section");

        assert_snapshot!(
                                                            tools_global_section_focus_summary(&params),
                                                            @r"
title: Settings / tools
subtitle: Browse and edit settings under `tools` in user config.toml.
Write scope: 1
Edit this section: 1
background_terminal_max_timeout: 1
tool_output_token_limit: 1
"
                                                        );
    }
}
