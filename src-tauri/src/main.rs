#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(serde::Serialize)]
struct HermesStatus {
    #[serde(rename = "nativeAvailable")]
    native_available: bool,
    #[serde(rename = "wslAvailable")]
    wsl_available: bool,
    #[serde(rename = "workspaceReady")]
    workspace_ready: bool,
    message: String,
    native: NativeHermesStatus,
    home: HermesHomeStatus,
    #[serde(rename = "codexHome")]
    codex_home: LocalHomeStatus,
    #[serde(rename = "wslDistros")]
    wsl_distros: Vec<WslDistroStatus>,
}

#[derive(serde::Serialize)]
struct NativeHermesStatus {
    available: bool,
    path: Option<String>,
    version: Option<String>,
}

#[derive(serde::Serialize)]
struct HermesHomeStatus {
    path: String,
    exists: bool,
    #[serde(rename = "hasConfig")]
    has_config: bool,
    #[serde(rename = "hasAuth")]
    has_auth: bool,
    #[serde(rename = "hasEnv")]
    has_env: bool,
    #[serde(rename = "hasSessions")]
    has_sessions: bool,
    #[serde(rename = "hasSkills")]
    has_skills: bool,
    #[serde(rename = "hasCron")]
    has_cron: bool,
    #[serde(rename = "hasKanban")]
    has_kanban: bool,
    #[serde(rename = "hasStateDatabase")]
    has_state_database: bool,
    #[serde(rename = "profileCount")]
    profile_count: usize,
}

#[derive(serde::Serialize)]
struct LocalHomeStatus {
    path: String,
    exists: bool,
}

#[derive(serde::Serialize)]
struct WslDistroStatus {
    name: String,
    #[serde(rename = "hermesCliAvailable")]
    hermes_cli_available: bool,
    #[serde(rename = "hermesHomeExists")]
    hermes_home_exists: bool,
    #[serde(rename = "linuxHome")]
    linux_home: Option<String>,
    #[serde(rename = "hermesHomePath")]
    hermes_home_path: Option<String>,
}

#[derive(serde::Serialize)]
struct HermesProfileCatalog {
    distro: String,
    profiles: Vec<HermesProfile>,
}

#[derive(Clone, serde::Serialize)]
struct HermesProfile {
    name: String,
    path: String,
    #[serde(rename = "isDefault")]
    is_default: bool,
    exists: bool,
}

#[derive(serde::Serialize)]
struct CreateHermesProfileResult {
    created: HermesProfile,
    catalog: HermesProfileCatalog,
    message: String,
}

#[derive(serde::Serialize)]
struct RepairHermesResult {
    distro: String,
    message: String,
    output: String,
}

#[derive(serde::Serialize)]
struct InstallHermesResult {
    distro: String,
    message: String,
    output: String,
}

#[derive(serde::Serialize)]
struct HermesRuntimeStatus {
    distro: String,
    profile: String,
    #[serde(rename = "hermesHome")]
    hermes_home: String,
    #[serde(rename = "hermesCommand")]
    hermes_command: Option<String>,
    version: Option<String>,
    #[serde(rename = "profileExists")]
    profile_exists: bool,
    #[serde(rename = "hasEnv")]
    has_env: bool,
    #[serde(rename = "hasConfig")]
    has_config: bool,
    #[serde(rename = "hasSessions")]
    has_sessions: bool,
    #[serde(rename = "hasSkills")]
    has_skills: bool,
    #[serde(rename = "hasCron")]
    has_cron: bool,
    #[serde(rename = "modelProvider")]
    model_provider: Option<String>,
    #[serde(rename = "modelDefault")]
    model_default: Option<String>,
    ready: bool,
    missing: Vec<String>,
    message: String,
}

#[derive(serde::Serialize)]
struct ConfigureHermesProviderResult {
    distro: String,
    profile: String,
    provider: String,
    model: String,
    message: String,
    output: String,
}

#[derive(serde::Serialize)]
struct ImportCodexAuthResult {
    distro: String,
    message: String,
}

#[derive(serde::Serialize)]
struct HermesCommandResult {
    distro: String,
    profile: String,
    output: String,
}

#[derive(serde::Serialize)]
struct ProfileCommandResult {
    distro: String,
    profile: String,
    command: String,
    output: String,
    #[serde(rename = "exitCode")]
    exit_code: i32,
}

struct CommandCapture {
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

struct HermesRuntime {
    hermes_home: String,
    hermes_command: String,
    path: String,
}

impl HermesRuntime {
    fn command_args(&self, hermes_args: Vec<String>) -> Vec<String> {
        let mut args = vec![
            "env".to_string(),
            format!("HERMES_HOME={}", self.hermes_home),
            format!("PATH={}", self.path),
            self.hermes_command.clone(),
        ];
        args.extend(hermes_args);
        args
    }

    fn shell_command_args(&self, command: &str) -> Vec<String> {
        let wrapped = format!(
            r#"set -o pipefail
if command -v timeout >/dev/null 2>&1; then
  timeout 45s bash -lc "$OS1_COMMAND"
  code=$?
else
  bash -lc "$OS1_COMMAND"
  code=$?
fi
printf '\n__OS1_EXIT__:%s\n' "$code"
exit 0"#
        );

        vec![
            "env".to_string(),
            format!("HERMES_HOME={}", self.hermes_home),
            format!("PATH={}", self.path),
            format!("OS1_COMMAND={command}"),
            "bash".to_string(),
            "-lc".to_string(),
            wrapped,
        ]
    }
}

#[derive(serde::Serialize)]
struct RealtimeKeyStatus {
    configured: bool,
    source: String,
}

async fn run_background<T, F>(task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| format!("Background task failed: {error}"))?
}

#[tauri::command]
async fn detect_hermes() -> Result<HermesStatus, String> {
    run_background(|| Ok(detect_hermes_blocking())).await
}

#[tauri::command]
async fn list_hermes_profiles(distro: Option<String>) -> Result<HermesProfileCatalog, String> {
    run_background(move || list_hermes_profiles_blocking(distro)).await
}

#[tauri::command]
async fn create_hermes_profile(
    distro: Option<String>,
    name: String,
    mode: String,
    clone_from: Option<String>,
) -> Result<CreateHermesProfileResult, String> {
    run_background(move || create_hermes_profile_blocking(distro, name, mode, clone_from)).await
}

#[tauri::command]
async fn repair_hermes(distro: Option<String>) -> Result<RepairHermesResult, String> {
    run_background(move || repair_hermes_blocking(distro)).await
}

#[tauri::command]
async fn install_hermes(distro: Option<String>) -> Result<InstallHermesResult, String> {
    run_background(move || install_hermes_blocking(distro)).await
}

#[tauri::command]
async fn check_hermes_runtime(
    distro: String,
    profile: String,
) -> Result<HermesRuntimeStatus, String> {
    run_background(move || check_hermes_runtime_blocking(distro, profile)).await
}

#[tauri::command]
async fn run_hermes_doctor(distro: String, profile: String) -> Result<HermesCommandResult, String> {
    run_background(move || run_hermes_doctor_blocking(distro, profile)).await
}

#[tauri::command]
async fn ask_hermes(
    distro: String,
    profile: String,
    prompt: String,
) -> Result<HermesCommandResult, String> {
    run_background(move || ask_hermes_blocking(distro, profile, prompt)).await
}

#[tauri::command]
async fn configure_hermes_provider(
    distro: String,
    profile: String,
    mode: String,
) -> Result<ConfigureHermesProviderResult, String> {
    run_background(move || configure_hermes_provider_blocking(distro, profile, mode)).await
}

#[tauri::command]
async fn import_codex_auth_to_wsl(
    distro: String,
    profile: String,
) -> Result<ImportCodexAuthResult, String> {
    run_background(move || import_codex_auth_to_wsl_blocking(distro, profile)).await
}

#[tauri::command]
async fn run_profile_command(
    distro: String,
    profile: String,
    command: String,
) -> Result<ProfileCommandResult, String> {
    run_background(move || run_profile_command_blocking(distro, profile, command)).await
}

fn detect_hermes_blocking() -> HermesStatus {
    let native = detect_native_hermes();
    let home = inspect_hermes_home(default_home_path(".hermes"));
    let codex_home = inspect_local_home(default_home_path(".codex"));
    let wsl_distros = detect_wsl_distros();
    let wsl_available = !wsl_distros.is_empty();
    let wsl_hermes_ready = wsl_distros
        .iter()
        .any(|distro| distro.hermes_cli_available || distro.hermes_home_exists);
    let workspace_ready = native.available || home.exists || wsl_hermes_ready || codex_home.exists;
    let message = summarize_hermes_detection(&native, &home, &codex_home, &wsl_distros);

    HermesStatus {
        native_available: native.available,
        wsl_available,
        workspace_ready,
        message,
        native,
        home,
        codex_home,
        wsl_distros,
    }
}

fn list_hermes_profiles_blocking(distro: Option<String>) -> Result<HermesProfileCatalog, String> {
    let distro = resolve_wsl_distro(distro)?;
    let home = run_wsl_capture_result(&distro, &["printenv", "HOME"])
        .map_err(|error| {
            format!(
                "Unable to resolve Linux home while listing Hermes profiles in {distro}. {}",
                summarize_command_error(&error)
            )
        })?
        .trim()
        .trim_end_matches('/')
        .to_string();
    if home.is_empty() {
        return Err(format!(
            "Unable to resolve Linux home while listing Hermes profiles in {distro}."
        ));
    }

    let base = format!("{home}/.hermes");
    let profiles_path = format!("{base}/profiles");
    let mut profiles = vec![HermesProfile {
        name: "default".to_string(),
        path: base.clone(),
        is_default: true,
        exists: run_wsl_status(&distro, vec!["test".to_string(), "-d".to_string(), base]),
    }];

    if run_wsl_status(
        &distro,
        vec!["test".to_string(), "-d".to_string(), profiles_path.clone()],
    ) {
        let output = run_wsl_capture_result_owned(
            &distro,
            vec![
                "find".to_string(),
                profiles_path,
                "-mindepth".to_string(),
                "1".to_string(),
                "-maxdepth".to_string(),
                "1".to_string(),
                "-type".to_string(),
                "d".to_string(),
                "-printf".to_string(),
                "%f\t%p\n".to_string(),
            ],
        )
        .map_err(|error| {
            format!(
                "Unable to list Hermes profiles in WSL distro {distro}. {}",
                summarize_command_error(&error)
            )
        })?;

        profiles.extend(output.lines().filter_map(|line| {
            let (name, path) = line.split_once('\t')?;
            Some(HermesProfile {
                name: name.to_string(),
                path: path.to_string(),
                is_default: false,
                exists: true,
            })
        }));
    }

    profiles.sort_by_key(|profile| (profile.is_default, profile.name.to_lowercase()));
    Ok(HermesProfileCatalog { distro, profiles })
}

fn create_hermes_profile_blocking(
    distro: Option<String>,
    name: String,
    mode: String,
    clone_from: Option<String>,
) -> Result<CreateHermesProfileResult, String> {
    let distro = resolve_wsl_distro(distro)?;
    let profile_name = validate_new_profile_name(&name)?;
    if !matches!(mode.as_str(), "blank" | "clone" | "cloneAll") {
        return Err("Unsupported profile creation mode.".to_string());
    }

    let clone_from = clone_from
        .map(|value| validate_existing_profile_name(&value))
        .transpose()?;
    if clone_from.is_some() && mode == "blank" {
        return Err("Clone source requires clone or cloneAll mode.".to_string());
    }

    let clone_source = clone_from.unwrap_or_else(|| "default".to_string());
    let home = run_wsl_capture_result(&distro, &["printenv", "HOME"])
        .map_err(|error| {
            format!(
                "Hermes could not resolve the Linux home directory in {distro}. {}",
                summarize_command_error(&error)
            )
        })?
        .trim()
        .trim_end_matches('/')
        .to_string();

    if home.is_empty() {
        return Err(format!(
            "Hermes could not resolve the Linux home directory in {distro}."
        ));
    }

    let base = format!("{home}/.hermes");
    let profiles = format!("{base}/profiles");
    let target = format!("{profiles}/{profile_name}");
    let source_home = if clone_source == "default" {
        base.clone()
    } else {
        format!("{profiles}/{clone_source}")
    };

    if run_wsl_status(
        &distro,
        vec!["test".to_string(), "-e".to_string(), target.clone()],
    ) {
        if run_wsl_status(
            &distro,
            vec!["test".to_string(), "-d".to_string(), target.clone()],
        ) {
            let catalog = list_hermes_profiles_blocking(Some(distro.clone()))?;
            let created = catalog
                .profiles
                .iter()
                .find(|profile| profile.name == profile_name)
                .cloned()
                .unwrap_or_else(|| HermesProfile {
                    name: profile_name.clone(),
                    path: target.clone(),
                    is_default: false,
                    exists: true,
                });
            return Ok(CreateHermesProfileResult {
                message: format!("Hermes profile {profile_name} already exists in {distro}"),
                created,
                catalog,
            });
        }
        return Err(format!(
            "Hermes profile path exists but is not a directory: {target}"
        ));
    }

    if (mode == "clone" || mode == "cloneAll")
        && !run_wsl_status(
            &distro,
            vec!["test".to_string(), "-d".to_string(), source_home.clone()],
        )
    {
        return Err(format!("Hermes clone source does not exist: {source_home}"));
    }

    run_wsl_profile_step(
        &distro,
        vec!["mkdir".to_string(), "-p".to_string(), target.clone()],
        &profile_name,
    )?;

    if mode == "cloneAll" {
        run_wsl_profile_step(
            &distro,
            vec![
                "cp".to_string(),
                "-a".to_string(),
                format!("{source_home}/."),
                format!("{target}/"),
            ],
            &profile_name,
        )?;
    } else if mode == "clone" {
        for item in ["config.yaml", ".env", "SOUL.md"] {
            let source = format!("{source_home}/{item}");
            if run_wsl_status(
                &distro,
                vec!["test".to_string(), "-f".to_string(), source.clone()],
            ) {
                run_wsl_profile_step(
                    &distro,
                    vec![
                        "cp".to_string(),
                        "-p".to_string(),
                        source,
                        format!("{target}/{item}"),
                    ],
                    &profile_name,
                )?;
            }
        }
    }

    let mut mkdir_args = vec!["mkdir".to_string(), "-p".to_string()];
    for child in ["cron", "sessions", "logs", "memories", "skills"] {
        mkdir_args.push(format!("{target}/{child}"));
    }
    run_wsl_profile_step(&distro, mkdir_args, &profile_name)?;

    ensure_profile_file(
        &distro,
        &profile_name,
        &format!("{target}/.env"),
        Some(&format!("{base}/hermes-agent/.env.example")),
    )?;
    ensure_profile_file(
        &distro,
        &profile_name,
        &format!("{target}/config.yaml"),
        Some(&format!("{base}/hermes-agent/cli-config.yaml.example")),
    )?;
    ensure_profile_file(&distro, &profile_name, &format!("{target}/SOUL.md"), None)?;

    let catalog = list_hermes_profiles_blocking(Some(distro.clone()))?;
    let Some(created) = catalog
        .profiles
        .iter()
        .find(|profile| profile.name == profile_name)
        .cloned()
    else {
        return Err(format!(
            "Hermes profile creation finished, but ~/.hermes/profiles/{profile_name} was not found in {distro}."
        ));
    };

    Ok(CreateHermesProfileResult {
        message: format!("Created OS1-managed Hermes profile {profile_name} in {distro}"),
        created,
        catalog,
    })
}

fn repair_hermes_blocking(distro: Option<String>) -> Result<RepairHermesResult, String> {
    let distro = resolve_wsl_distro(distro)?;
    let script = r#"set -eu
install="$HOME/.hermes/hermes-agent"
uv="$HOME/.local/bin/uv"
if ! test -x "$install/hermes"; then
  printf 'Hermes checkout was not found at %s' "$install" >&2
  exit 127
fi
if ! test -x "$uv"; then
  printf 'uv was not found at %s. Install uv or rerun the Hermes installer.' "$uv" >&2
  exit 127
fi
cd "$install"
if ! test -x "$install/venv/bin/python"; then
  env -u VIRTUAL_ENV "$uv" venv venv --python 3.11
fi
env -u VIRTUAL_ENV "$uv" pip install --python "$install/venv/bin/python" -e .
if test -f "$install/mini-swe-agent/pyproject.toml"; then
  env -u VIRTUAL_ENV "$uv" pip install --python "$install/venv/bin/python" -e "$install/mini-swe-agent"
fi
mkdir -p "$HOME/.local/bin"
ln -sf "$install/venv/bin/hermes" "$HOME/.local/bin/hermes"
"$HOME/.local/bin/hermes" --version
"#;

    let output = run_wsl_capture_result(&distro, &["sh", "-lc", script]).map_err(|error| {
        format!(
            "Hermes repair failed in {distro}. {}",
            summarize_command_error(&error)
        )
    })?;

    Ok(RepairHermesResult {
        distro: distro.clone(),
        message: format!("Hermes repaired in {distro}"),
        output: compact_tail(&output, 8),
    })
}

fn install_hermes_blocking(distro: Option<String>) -> Result<InstallHermesResult, String> {
    let distro = resolve_wsl_distro(distro)?;
    let script = r#"set -eu
export PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$HOME/.local/bin:$PATH"
if ! command -v curl >/dev/null 2>&1; then
  printf 'curl is required to install Hermes inside WSL.' >&2
  exit 127
fi
curl -fsSL https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.sh | bash
export PATH="$HOME/.local/bin:$HOME/.hermes/hermes-agent/venv/bin:$PATH"
if command -v hermes >/dev/null 2>&1; then
  hermes --version
elif test -x "$HOME/.local/bin/hermes"; then
  "$HOME/.local/bin/hermes" --version
elif test -x "$HOME/.hermes/hermes-agent/venv/bin/hermes"; then
  "$HOME/.hermes/hermes-agent/venv/bin/hermes" --version
else
  printf 'Hermes installer finished, but the Hermes CLI was not found.' >&2
  exit 127
fi
"#;

    let output = run_wsl_capture_result(&distro, &["sh", "-lc", script]).map_err(|error| {
        format!(
            "Hermes install failed in {distro}. {}",
            summarize_command_error(&error)
        )
    })?;

    Ok(InstallHermesResult {
        distro: distro.clone(),
        message: format!("Hermes installed in {distro}"),
        output: compact_tail(&output, 12),
    })
}

fn check_hermes_runtime_blocking(distro: String, profile: String) -> Result<HermesRuntimeStatus, String> {
    let distro = resolve_wsl_distro(Some(distro))?;
    let profile = validate_profile_name_for_runtime(&profile)?;
    inspect_hermes_runtime(&distro, &profile)
}

fn run_hermes_doctor_blocking(distro: String, profile: String) -> Result<HermesCommandResult, String> {
    let distro = resolve_wsl_distro(Some(distro))?;
    let profile = validate_profile_name_for_runtime(&profile)?;
    let runtime = resolve_hermes_runtime(&distro, &profile)?;
    let output =
        run_wsl_capture_result_owned(&distro, runtime.command_args(vec!["doctor".to_string()]))
            .map_err(|error| {
                format!(
                    "Hermes doctor failed in {distro}. {}",
                    summarize_command_error(&error)
                )
            })?;
    Ok(HermesCommandResult {
        distro,
        profile,
        output: compact_tail(&output, 30),
    })
}

fn ask_hermes_blocking(
    distro: String,
    profile: String,
    prompt: String,
) -> Result<HermesCommandResult, String> {
    let distro = resolve_wsl_distro(Some(distro))?;
    let profile = validate_profile_name_for_runtime(&profile)?;
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("Prompt is required.".to_string());
    }
    if prompt.chars().count() > 8000 {
        return Err("Prompt is too long for this Hermes bridge call.".to_string());
    }

    let runtime = resolve_hermes_runtime(&distro, &profile)?;
    let output = run_wsl_capture_result_owned(
        &distro,
        runtime.command_args(vec![
            "chat".to_string(),
            "--quiet".to_string(),
            "--query".to_string(),
            prompt.to_string(),
        ]),
    )
    .map_err(|error| {
        format!(
            "Hermes chat failed in {distro}. {}",
            summarize_command_error(&error)
        )
    })?;
    Ok(HermesCommandResult {
        distro,
        profile,
        output: compact_tail(&output, 80),
    })
}

fn configure_hermes_provider_blocking(
    distro: String,
    profile: String,
    mode: String,
) -> Result<ConfigureHermesProviderResult, String> {
    let profile = validate_profile_name_for_runtime(&profile)?;
    let runtime = resolve_hermes_runtime(&distro, &profile)?;

    match mode.as_str() {
        "openai-key" => configure_openai_key_provider(&distro, &profile, &runtime),
        "codex-subscription" => configure_codex_subscription_provider(&distro, &profile, &runtime),
        _ => Err("Unsupported Hermes provider setup option.".to_string()),
    }
}

fn import_codex_auth_to_wsl_blocking(distro: String, profile: String) -> Result<ImportCodexAuthResult, String> {
    let distro = resolve_wsl_distro(Some(distro))?;
    let profile = validate_profile_name_for_runtime(&profile)?;
    let runtime = resolve_hermes_runtime(&distro, &profile)?;
    let user_profile = std::env::var("USERPROFILE")
        .map_err(|_| "Windows USERPROFILE was not available.".to_string())?;
    let auth_path = Path::new(&user_profile).join(".codex").join("auth.json");
    let auth_json = fs::read_to_string(&auth_path).map_err(|error| {
        format!(
            "Codex auth was not found at {}. {}",
            auth_path.display(),
            error
        )
    })?;
    serde_json::from_str::<serde_json::Value>(&auth_json)
        .map_err(|_| "Codex auth file is not valid JSON.".to_string())?;

    let script = r#"
import json
import os
import stat
import sys
from datetime import datetime, timezone
from pathlib import Path

payload = json.loads(sys.stdin.read())
tokens = payload.get("tokens")
if not isinstance(tokens, dict) or not tokens.get("access_token") or not tokens.get("refresh_token"):
    raise SystemExit("Codex auth file does not contain usable OAuth tokens.")

home = Path.home()
codex_dir = home / ".codex"
codex_dir.mkdir(parents=True, exist_ok=True)
codex_target = codex_dir / "auth.json"
codex_target.write_text(json.dumps(payload, indent=2) + "\n")
os.chmod(codex_target, stat.S_IRUSR | stat.S_IWUSR)

hermes_home = Path(os.environ["HERMES_HOME"])
hermes_home.mkdir(parents=True, exist_ok=True)
auth_target = hermes_home / "auth.json"
if auth_target.exists() and auth_target.read_text().strip():
    try:
        auth_store = json.loads(auth_target.read_text())
    except Exception:
        auth_store = {}
else:
    auth_store = {}
if not isinstance(auth_store, dict):
    auth_store = {}
providers = auth_store.get("providers")
if not isinstance(providers, dict):
    providers = {}
providers["openai-codex"] = {
    "tokens": tokens,
    "last_refresh": payload.get("last_refresh") or datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "auth_mode": "chatgpt",
}
auth_store["version"] = auth_store.get("version") or 1
auth_store["providers"] = providers
auth_store["active_provider"] = "openai-codex"
auth_store["updated_at"] = datetime.now(timezone.utc).isoformat()
auth_target.write_text(json.dumps(auth_store, indent=2) + "\n")
os.chmod(auth_target, stat.S_IRUSR | stat.S_IWUSR)
print("Imported Codex auth into Hermes profile")
"#;

    run_wsl_capture_with_stdin(
        &distro,
        vec![
            "env".to_string(),
            format!("HERMES_HOME={}", runtime.hermes_home),
            "python3".to_string(),
            "-c".to_string(),
            script.to_string(),
        ],
        &auth_json,
    )
    .map_err(|error| {
        format!(
            "Codex auth could not be imported into {distro}. {}",
            summarize_command_error(&error)
        )
    })?;

    Ok(ImportCodexAuthResult {
        distro,
        message: "Imported Windows Codex login into the Hermes profile.".to_string(),
    })
}

fn configure_openai_key_provider(
    distro: &str,
    profile: &str,
    runtime: &HermesRuntime,
) -> Result<ConfigureHermesProviderResult, String> {
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| "OPENAI_API_KEY is not configured for OS1.".to_string())?;
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return Err("OPENAI_API_KEY is empty.".to_string());
    }

    write_provider_config(
        distro,
        runtime,
        &serde_json::json!({
            "mode": "openai-key",
            "apiKey": api_key,
            "model": "gpt-5.5",
            "provider": "openai",
            "baseUrl": "https://api.openai.com/v1",
        })
        .to_string(),
    )?;

    let output = verify_hermes_chat(distro, runtime)?;
    Ok(ConfigureHermesProviderResult {
        distro: distro.to_string(),
        profile: profile.to_string(),
        provider: "openai".to_string(),
        model: "gpt-5.5".to_string(),
        message: "Hermes will use the OS1 OpenAI key with GPT-5.5.".to_string(),
        output,
    })
}

fn configure_codex_subscription_provider(
    distro: &str,
    profile: &str,
    runtime: &HermesRuntime,
) -> Result<ConfigureHermesProviderResult, String> {
    let home = linux_home(distro).map_err(|error| {
        format!(
            "Hermes could not inspect Codex credentials in {distro}. {}",
            summarize_command_error(&error)
        )
    })?;
    let codex_auth_exists = run_wsl_status(
        distro,
        vec![
            "test".to_string(),
            "-f".to_string(),
            format!("{home}/.codex/auth.json"),
        ],
    ) || run_wsl_status(
        distro,
        vec![
            "test".to_string(),
            "-f".to_string(),
            format!("{}/auth.json", runtime.hermes_home),
        ],
    );

    if !codex_auth_exists {
        return Err(
            "Codex subscription setup needs a one-time ChatGPT OAuth login. OS1 can automate this after we add the device-code screen, or it can import ~/.codex/auth.json when present in WSL."
                .to_string(),
        );
    }

    write_provider_config(
        distro,
        runtime,
        &serde_json::json!({
            "mode": "codex-subscription",
            "model": "gpt-5.5",
            "provider": "openai-codex",
        })
        .to_string(),
    )?;

    let auth_status = run_wsl_capture_result_owned(
        distro,
        runtime.command_args(vec![
            "auth".to_string(),
            "status".to_string(),
            "openai-codex".to_string(),
        ]),
    )
    .map_err(|error| {
        format!(
            "Codex subscription auth could not be checked in {distro}. {}",
            summarize_command_error(&error)
        )
    })?;

    if auth_status.to_lowercase().contains("logged out") {
        return Err("Codex subscription is not authenticated yet. Run the one-time ChatGPT OAuth flow before entering OS1 with this option.".to_string());
    }

    let output = verify_hermes_chat(distro, runtime)?;
    Ok(ConfigureHermesProviderResult {
        distro: distro.to_string(),
        profile: profile.to_string(),
        provider: "openai-codex".to_string(),
        model: "gpt-5.5".to_string(),
        message: "Hermes will use the Codex subscription provider.".to_string(),
        output,
    })
}

fn write_provider_config(
    distro: &str,
    runtime: &HermesRuntime,
    payload: &str,
) -> Result<(), String> {
    let script = r#"
import json
import os
import stat
import sys
from pathlib import Path

import yaml

payload = json.loads(sys.stdin.read())
home = Path(os.environ["HERMES_HOME"])
home.mkdir(parents=True, exist_ok=True)
env_path = home / ".env"
config_path = home / "config.yaml"

def quote_env(value):
    return '"' + str(value).replace("\\", "\\\\").replace('"', '\\"').replace("\n", "") + '"'

if payload.get("mode") == "openai-key":
    lines = env_path.read_text().splitlines() if env_path.exists() else []
    next_lines = []
    replaced = False
    for line in lines:
        if line.startswith("OPENAI_API_KEY="):
            next_lines.append("OPENAI_API_KEY=" + quote_env(payload["apiKey"]))
            replaced = True
        else:
            next_lines.append(line)
    if not replaced:
        next_lines.append("OPENAI_API_KEY=" + quote_env(payload["apiKey"]))
    env_path.write_text("\n".join(next_lines).rstrip() + "\n")
    os.chmod(env_path, stat.S_IRUSR | stat.S_IWUSR)

data = {}
if config_path.exists() and config_path.read_text().strip():
    data = yaml.safe_load(config_path.read_text()) or {}
if not isinstance(data, dict):
    data = {}

model = data.get("model") if isinstance(data.get("model"), dict) else {}
model = dict(model)
model["default"] = payload["model"]
model["provider"] = payload["provider"]
model.pop("api_key", None)
model.pop("api_mode", None)

if payload.get("mode") == "openai-key":
    provider = {
        "name": "openai",
        "base_url": payload["baseUrl"],
        "key_env": "OPENAI_API_KEY",
    }
    providers = data.get("custom_providers")
    if not isinstance(providers, list):
        providers = []
    for index, item in enumerate(providers):
        if isinstance(item, dict) and item.get("name") == "openai":
            providers[index] = provider
            break
    else:
        providers.append(provider)
    data["custom_providers"] = providers
    model["base_url"] = payload["baseUrl"]
else:
    model.pop("base_url", None)

data["model"] = model
tmp_path = config_path.with_suffix(".yaml.tmp")
tmp_path.write_text(yaml.safe_dump(data, sort_keys=False, default_flow_style=False))
tmp_path.replace(config_path)
os.chmod(config_path, stat.S_IRUSR | stat.S_IWUSR)
print(f"Configured {payload['provider']} / {payload['model']}")
"#;

    let python = format!(
        "{}/.hermes/hermes-agent/venv/bin/python",
        linux_home(distro).map_err(|error| {
            format!(
                "Hermes could not resolve Python for provider setup in {distro}. {}",
                summarize_command_error(&error)
            )
        })?
    );

    let output = run_wsl_capture_with_stdin(
        distro,
        vec![
            "env".to_string(),
            format!("HERMES_HOME={}", runtime.hermes_home),
            format!("PATH={}", runtime.path),
            python,
            "-c".to_string(),
            script.to_string(),
        ],
        payload,
    )
    .map_err(|error| {
        format!(
            "Hermes provider config could not be written in {distro}. {}",
            summarize_command_error(&error)
        )
    })?;

    if output.trim().is_empty() {
        return Ok(());
    }
    Ok(())
}

fn verify_hermes_chat(distro: &str, runtime: &HermesRuntime) -> Result<String, String> {
    run_wsl_capture_result_owned(
        distro,
        runtime.command_args(vec![
            "chat".to_string(),
            "--quiet".to_string(),
            "--query".to_string(),
            "Reply with exactly: Hermes provider ok".to_string(),
        ]),
    )
    .map_err(|error| {
        format!(
            "Hermes provider was configured, but chat verification failed in {distro}. {}",
            summarize_command_error(&error)
        )
    })
}

fn run_profile_command_blocking(
    distro: String,
    profile: String,
    command: String,
) -> Result<ProfileCommandResult, String> {
    let distro = resolve_wsl_distro(Some(distro))?;
    let profile = validate_profile_name_for_runtime(&profile)?;
    let command = command.trim();
    if command.is_empty() {
        return Err("Command is required.".to_string());
    }
    if command.chars().count() > 4000 {
        return Err("Command is too long for this console.".to_string());
    }

    let runtime = resolve_hermes_runtime(&distro, &profile)?;
    let output = run_wsl_capture_result_owned(&distro, runtime.shell_command_args(command))
        .map_err(|error| {
            format!(
                "Profile command failed in {distro}. {}",
                summarize_command_error(&error)
            )
        })?;
    let (output, exit_code) = parse_profile_command_output(&output);

    Ok(ProfileCommandResult {
        distro,
        profile,
        command: command.to_string(),
        output: compact_text(&output, 12000),
        exit_code,
    })
}

fn detect_native_hermes() -> NativeHermesStatus {
    let path = find_executable("hermes");
    let version = path
        .as_deref()
        .and_then(|executable| {
            run_command_capture(executable, &["version"])
                .or_else(|| run_command_capture(executable, &["--version"]))
        })
        .map(|output| compact_single_line(&output));

    NativeHermesStatus {
        available: path.is_some(),
        path,
        version,
    }
}

fn inspect_hermes_home(path: PathBuf) -> HermesHomeStatus {
    let profiles_path = path.join("profiles");

    HermesHomeStatus {
        path: path_to_string(&path),
        exists: path.is_dir(),
        has_config: path.join("config.yaml").is_file() || path.join("config.yml").is_file(),
        has_auth: path.join("auth.json").is_file(),
        has_env: path.join(".env").is_file(),
        has_sessions: path.join("sessions").is_dir(),
        has_skills: path.join("skills").is_dir(),
        has_cron: path.join("cron").join("jobs.json").is_file(),
        has_kanban: path.join("kanban.db").is_file(),
        has_state_database: [
            "state.db",
            "state.sqlite",
            "state.sqlite3",
            "store.db",
            "store.sqlite",
            "store.sqlite3",
        ]
        .iter()
        .any(|file_name| path.join(file_name).is_file()),
        profile_count: count_child_directories(&profiles_path),
    }
}

fn inspect_local_home(path: PathBuf) -> LocalHomeStatus {
    LocalHomeStatus {
        path: path_to_string(&path),
        exists: path.is_dir(),
    }
}

fn detect_wsl_distros() -> Vec<WslDistroStatus> {
    let Some(output) = run_command_capture("wsl.exe", &["--list", "--quiet"]) else {
        return Vec::new();
    };

    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.eq_ignore_ascii_case("docker-desktop"))
        .filter(|line| !line.eq_ignore_ascii_case("docker-desktop-data"))
        .take(8)
        .map(inspect_wsl_distro)
        .collect()
}

fn inspect_wsl_distro(name: &str) -> WslDistroStatus {
    let script = concat!(
        "printf 'home='; printf '%s' \"$HOME\"; ",
        "printf '\\ncli='; if command -v hermes >/dev/null 2>&1 || test -x \"$HOME/.local/bin/hermes\" || test -x \"$HOME/.hermes/hermes-agent/venv/bin/hermes\" || test -x \"$HOME/.hermes/hermes-agent/hermes\"; then printf yes; else printf no; fi; ",
        "printf '\\nhermes_home='; if test -d \"$HOME/.hermes\"; then printf yes; else printf no; fi"
    );
    let output = run_command_capture("wsl.exe", &["-d", name, "--", "sh", "-lc", script])
        .unwrap_or_default();
    let linux_home = parse_keyed_output(&output, "home").filter(|value| !value.is_empty());
    let hermes_cli_available =
        parse_keyed_output(&output, "cli").is_some_and(|value| value == "yes");
    let hermes_home_exists =
        parse_keyed_output(&output, "hermes_home").is_some_and(|value| value == "yes");
    let hermes_home_path = linux_home.as_ref().map(|home| format!("{home}/.hermes"));

    WslDistroStatus {
        name: name.to_string(),
        hermes_cli_available,
        hermes_home_exists,
        linux_home,
        hermes_home_path,
    }
}

fn summarize_hermes_detection(
    native: &NativeHermesStatus,
    home: &HermesHomeStatus,
    codex_home: &LocalHomeStatus,
    wsl_distros: &[WslDistroStatus],
) -> String {
    let ready_wsl = wsl_distros
        .iter()
        .find(|distro| distro.hermes_cli_available || distro.hermes_home_exists);

    if native.available && home.exists {
        return "Native Hermes found; local ~/.hermes ready".to_string();
    }
    if native.available {
        return "Native Hermes found; no local ~/.hermes yet".to_string();
    }
    if home.exists {
        return "Local ~/.hermes found; Hermes CLI not on PATH".to_string();
    }
    if let Some(distro) = ready_wsl {
        return format!("WSL {} has Hermes signals", distro.name);
    }
    if !wsl_distros.is_empty() {
        return format!(
            "WSL available; no Hermes home found across {} distro(s)",
            wsl_distros.len()
        );
    }
    if codex_home.exists {
        return "Codex home found; Hermes not detected yet".to_string();
    }
    "No local Hermes workspace detected yet".to_string()
}

fn default_home_path(child: &str) -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(child)
}

fn find_executable(command_name: &str) -> Option<String> {
    let path_output = run_command_capture("where.exe", &[command_name]);
    if let Some(path) = path_output
        .as_deref()
        .and_then(|output| output.lines().map(str::trim).find(|line| !line.is_empty()))
    {
        return Some(path.to_string());
    }

    let path_var = std::env::var_os("PATH")?;
    let extensions = executable_extensions();
    for directory in std::env::split_paths(&path_var) {
        for extension in &extensions {
            let candidate = directory.join(format!("{command_name}{extension}"));
            if candidate.is_file() {
                return Some(path_to_string(&candidate));
            }
        }
    }

    None
}

fn executable_extensions() -> Vec<String> {
    let mut extensions = vec![
        "".to_string(),
        ".exe".to_string(),
        ".cmd".to_string(),
        ".bat".to_string(),
    ];
    if let Some(pathext) = std::env::var_os("PATHEXT") {
        for extension in pathext.to_string_lossy().split(';') {
            let normalized = extension.trim().to_ascii_lowercase();
            if !normalized.is_empty() && !extensions.iter().any(|item| item == &normalized) {
                extensions.push(normalized);
            }
        }
    }
    extensions
}

fn run_command_capture(program: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args);
    hide_process_window(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let mut combined = decode_command_output(&output.stdout);
    let stderr = decode_command_output(&output.stderr);
    if !stderr.trim().is_empty() {
        if !combined.trim().is_empty() {
            combined.push('\n');
        }
        combined.push_str(stderr.trim());
    }
    Some(combined)
}

#[cfg(windows)]
fn hide_process_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_process_window(_command: &mut Command) {}

fn decode_command_output(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes.iter().take(32).filter(|byte| **byte == 0).count() > 4 {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16_lossy(&units)
            .trim_matches('\u{feff}')
            .replace('\0', "")
    } else {
        String::from_utf8_lossy(bytes).replace('\0', "")
    }
}

fn parse_keyed_output(output: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix(&prefix)
            .map(|value| value.trim().to_string())
    })
}

fn compact_single_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string()
}

fn compact_tail(value: &str, max_lines: usize) -> String {
    let mut lines: Vec<_> = value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.len() > max_lines {
        lines = lines.split_off(lines.len() - max_lines);
    }
    lines.join("\n")
}

fn compact_text(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    let char_count = trimmed.chars().count();
    if char_count <= max_chars {
        return trimmed.to_string();
    }
    let start = char_count - max_chars;
    trimmed.chars().skip(start).collect()
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn count_child_directories(path: &Path) -> usize {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
                .count()
        })
        .unwrap_or(0)
}

fn resolve_wsl_distro(provided: Option<String>) -> Result<String, String> {
    if let Some(value) = provided {
        let value = value.trim();
        if !value.is_empty() {
            return Ok(value.to_string());
        }
    }

    let distros = detect_wsl_distros();
    if let Some(distro) = distros
        .iter()
        .find(|distro| distro.hermes_cli_available || distro.hermes_home_exists)
        .or_else(|| distros.first())
    {
        return Ok(distro.name.clone());
    }

    Err("No WSL distro was detected.".to_string())
}

fn validate_new_profile_name(value: &str) -> Result<String, String> {
    let name = validate_existing_profile_name(value)?;
    if name == "default" {
        return Err("default is reserved for the base Hermes profile.".to_string());
    }
    Ok(name)
}

fn validate_existing_profile_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.is_empty() {
        return Err("Profile name is required.".to_string());
    }
    if name == "." || name == ".." {
        return Err("Profile name must be a name, not a path.".to_string());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(
            "Profile names can use letters, numbers, hyphens, and underscores.".to_string(),
        );
    }
    Ok(name.to_string())
}

fn validate_profile_name_for_runtime(value: &str) -> Result<String, String> {
    let name = validate_existing_profile_name(value)?;
    Ok(name)
}

fn resolve_hermes_runtime(distro: &str, profile: &str) -> Result<HermesRuntime, String> {
    let home = linux_home(distro).map_err(|error| {
        format!(
            "Hermes runtime check failed in {distro}. {}",
            summarize_command_error(&error)
        )
    })?;
    let hermes_home = hermes_home_for_profile(&home, profile);
    let path = hermes_runtime_path(&home);
    let hermes_command = find_hermes_command(distro, &home)
        .ok_or_else(|| format!("Hermes CLI was not found in WSL distro {distro}."))?;

    Ok(HermesRuntime {
        hermes_home,
        hermes_command,
        path,
    })
}

fn inspect_hermes_runtime(distro: &str, profile: &str) -> Result<HermesRuntimeStatus, String> {
    let home = linux_home(distro).map_err(|error| {
        format!(
            "Hermes runtime check failed in {distro}. {}",
            summarize_command_error(&error)
        )
    })?;
    let hermes_home = hermes_home_for_profile(&home, profile);
    let hermes_command = find_hermes_command(distro, &home);

    let version = hermes_command.as_ref().and_then(|command| {
        run_wsl_capture_result_owned(
            distro,
            vec![
                "env".to_string(),
                format!("HERMES_HOME={hermes_home}"),
                format!("PATH={}", hermes_runtime_path(&home)),
                command.clone(),
                "--version".to_string(),
            ],
        )
        .ok()
        .and_then(|output| {
            output
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(str::to_string)
        })
    });

    let profile_exists = run_wsl_status(
        distro,
        vec!["test".to_string(), "-d".to_string(), hermes_home.clone()],
    );
    let has_env = run_wsl_status(
        distro,
        vec![
            "test".to_string(),
            "-f".to_string(),
            format!("{hermes_home}/.env"),
        ],
    );
    let has_config = run_wsl_status(
        distro,
        vec![
            "test".to_string(),
            "-f".to_string(),
            format!("{hermes_home}/config.yaml"),
        ],
    ) || run_wsl_status(
        distro,
        vec![
            "test".to_string(),
            "-f".to_string(),
            format!("{hermes_home}/config.yml"),
        ],
    );
    let has_sessions = run_wsl_status(
        distro,
        vec![
            "test".to_string(),
            "-d".to_string(),
            format!("{hermes_home}/sessions"),
        ],
    );
    let has_skills = run_wsl_status(
        distro,
        vec![
            "test".to_string(),
            "-d".to_string(),
            format!("{hermes_home}/skills"),
        ],
    );
    let has_cron = run_wsl_status(
        distro,
        vec![
            "test".to_string(),
            "-d".to_string(),
            format!("{hermes_home}/cron"),
        ],
    );
    let (model_provider, model_default) = read_profile_model_config(distro, &hermes_home);

    build_hermes_runtime_status(
        distro,
        profile,
        hermes_home,
        hermes_command,
        version,
        profile_exists,
        has_env,
        has_config,
        has_sessions,
        has_skills,
        has_cron,
        model_provider,
        model_default,
    )
}

fn linux_home(distro: &str) -> Result<String, CommandCapture> {
    let home = run_wsl_capture_result(distro, &["printenv", "HOME"])?
        .trim()
        .trim_end_matches('/')
        .to_string();
    if home.is_empty() {
        return Err(CommandCapture {
            stdout: String::new(),
            stderr: "Linux home was empty.".to_string(),
            code: None,
        });
    }
    Ok(home)
}

fn hermes_home_for_profile(home: &str, profile: &str) -> String {
    if profile == "default" {
        format!("{home}/.hermes")
    } else {
        format!("{home}/.hermes/profiles/{profile}")
    }
}

fn hermes_runtime_path(home: &str) -> String {
    format!(
        "{home}/.local/bin:{home}/.hermes/hermes-agent/venv/bin:{home}/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
    )
}

fn find_hermes_command(distro: &str, home: &str) -> Option<String> {
    [
        format!("{home}/.local/bin/hermes"),
        format!("{home}/.hermes/hermes-agent/venv/bin/hermes"),
        format!("{home}/.hermes/hermes-agent/hermes"),
    ]
    .into_iter()
    .find(|candidate| {
        run_wsl_status(
            distro,
            vec!["test".to_string(), "-x".to_string(), candidate.clone()],
        )
    })
}

fn read_profile_model_config(distro: &str, hermes_home: &str) -> (Option<String>, Option<String>) {
    let script = r#"import json
import os
from pathlib import Path

import yaml

home = Path(os.environ["HERMES_HOME"])
config_path = home / "config.yaml"
if not config_path.exists():
    config_path = home / "config.yml"
data = {}
if config_path.exists() and config_path.read_text().strip():
    loaded = yaml.safe_load(config_path.read_text()) or {}
    if isinstance(loaded, dict):
        data = loaded
model = data.get("model")
provider = None
default = None
if isinstance(model, dict):
    provider = model.get("provider")
    default = model.get("default") or model.get("model")
elif isinstance(model, str):
    default = model
print(json.dumps({"provider": provider, "default": default}))
"#;

    let Ok(output) = run_wsl_capture_result_owned(
        distro,
        vec![
            "env".to_string(),
            format!("HERMES_HOME={hermes_home}"),
            "python3".to_string(),
            "-c".to_string(),
            script.to_string(),
        ],
    ) else {
        return (None, None);
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(output.trim()) else {
        return (None, None);
    };
    let provider = value
        .get("provider")
        .and_then(|item| item.as_str())
        .map(str::to_string);
    let default = value
        .get("default")
        .and_then(|item| item.as_str())
        .map(str::to_string);
    (provider, default)
}

fn build_hermes_runtime_status(
    distro: &str,
    profile: &str,
    hermes_home: String,
    hermes_command: Option<String>,
    version: Option<String>,
    profile_exists: bool,
    has_env: bool,
    has_config: bool,
    has_sessions: bool,
    has_skills: bool,
    has_cron: bool,
    model_provider: Option<String>,
    model_default: Option<String>,
) -> Result<HermesRuntimeStatus, String> {
    let mut missing = Vec::new();
    if hermes_command.is_none() {
        missing.push("Hermes CLI".to_string());
    }
    if !profile_exists {
        missing.push("profile home".to_string());
    }
    if !has_env {
        missing.push(".env".to_string());
    }
    if !has_config {
        missing.push("config.yaml".to_string());
    }
    if !has_sessions {
        missing.push("sessions folder".to_string());
    }
    if !has_skills {
        missing.push("skills folder".to_string());
    }
    if !has_cron {
        missing.push("cron folder".to_string());
    }

    let ready =
        hermes_command.is_some() && profile_exists && has_sessions && has_skills && has_cron;
    let message = if ready {
        format!("{profile} is ready in {distro}")
    } else if missing.is_empty() {
        format!("{profile} needs attention in {distro}")
    } else {
        format!("{profile} needs {}", missing.join(", "))
    };

    Ok(HermesRuntimeStatus {
        distro: distro.to_string(),
        profile: profile.to_string(),
        hermes_home,
        hermes_command,
        version,
        profile_exists,
        has_env,
        has_config,
        has_sessions,
        has_skills,
        has_cron,
        model_provider,
        model_default,
        ready,
        missing,
        message,
    })
}

fn parse_profile_command_output(output: &str) -> (String, i32) {
    let marker = "__OS1_EXIT__:";
    let mut exit_code = 0;
    let mut lines: Vec<&str> = output.lines().collect();

    if let Some(index) = lines
        .iter()
        .rposition(|line| line.trim().starts_with(marker))
    {
        let marker_line = lines[index].trim();
        if let Some(value) = marker_line.strip_prefix(marker) {
            exit_code = value.trim().parse::<i32>().unwrap_or(1);
        }
        lines.truncate(index);
    }

    (lines.join("\n"), exit_code)
}

fn run_wsl_capture_result(distro: &str, args: &[&str]) -> Result<String, CommandCapture> {
    let mut owned = vec!["-d".to_string(), distro.to_string(), "--".to_string()];
    owned.extend(args.iter().map(|arg| arg.to_string()));
    run_command_capture_owned_result("wsl.exe", &owned)
}

fn run_wsl_capture_result_owned(distro: &str, args: Vec<String>) -> Result<String, CommandCapture> {
    let mut owned = vec!["-d".to_string(), distro.to_string(), "--".to_string()];
    owned.extend(args);
    run_command_capture_owned_result("wsl.exe", &owned)
}

fn run_wsl_capture_with_stdin(
    distro: &str,
    args: Vec<String>,
    stdin_text: &str,
) -> Result<String, CommandCapture> {
    let mut owned = vec!["-d".to_string(), distro.to_string(), "--".to_string()];
    owned.extend(args);
    run_command_capture_owned_with_stdin_result("wsl.exe", &owned, stdin_text)
}

fn run_wsl_status(distro: &str, args: Vec<String>) -> bool {
    run_wsl_capture_result_owned(distro, args).is_ok()
}

fn run_wsl_profile_step(
    distro: &str,
    args: Vec<String>,
    profile_name: &str,
) -> Result<String, String> {
    run_wsl_capture_result_owned(distro, args).map_err(|error| {
        format!(
            "Hermes could not create profile {profile_name} in {distro}. {}",
            summarize_command_error(&error)
        )
    })
}

fn ensure_profile_file(
    distro: &str,
    profile_name: &str,
    target: &str,
    source: Option<&str>,
) -> Result<(), String> {
    if run_wsl_status(
        distro,
        vec!["test".to_string(), "-f".to_string(), target.to_string()],
    ) {
        return Ok(());
    }

    if let Some(source) = source {
        if run_wsl_status(
            distro,
            vec!["test".to_string(), "-f".to_string(), source.to_string()],
        ) {
            run_wsl_profile_step(
                distro,
                vec!["cp".to_string(), source.to_string(), target.to_string()],
                profile_name,
            )?;
            return Ok(());
        }
    }

    run_wsl_profile_step(
        distro,
        vec!["touch".to_string(), target.to_string()],
        profile_name,
    )?;
    Ok(())
}

fn run_command_capture_owned_result(
    program: &str,
    args: &[String],
) -> Result<String, CommandCapture> {
    let mut command = Command::new(program);
    command.args(args);
    hide_process_window(&mut command);
    let output = command.output().map_err(|error| CommandCapture {
        stdout: String::new(),
        stderr: error.to_string(),
        code: None,
    })?;

    let stdout = decode_command_output(&output.stdout);
    let stderr = decode_command_output(&output.stderr);
    if output.status.success() {
        let mut combined = stdout.clone();
        if !stderr.trim().is_empty() {
            if !combined.trim().is_empty() {
                combined.push('\n');
            }
            combined.push_str(stderr.trim());
        }
        return Ok(combined);
    }

    Err(CommandCapture {
        stdout,
        stderr,
        code: output.status.code(),
    })
}

fn run_command_capture_owned_with_stdin_result(
    program: &str,
    args: &[String],
    stdin_text: &str,
) -> Result<String, CommandCapture> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_process_window(&mut command);

    let mut child = command.spawn().map_err(|error| CommandCapture {
        stdout: String::new(),
        stderr: error.to_string(),
        code: None,
    })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(stdin_text.as_bytes())
            .map_err(|error| CommandCapture {
                stdout: String::new(),
                stderr: error.to_string(),
                code: None,
            })?;
    }

    let output = child.wait_with_output().map_err(|error| CommandCapture {
        stdout: String::new(),
        stderr: error.to_string(),
        code: None,
    })?;

    let stdout = decode_command_output(&output.stdout);
    let stderr = decode_command_output(&output.stderr);
    if output.status.success() {
        let mut combined = stdout.clone();
        if !stderr.trim().is_empty() {
            if !combined.trim().is_empty() {
                combined.push('\n');
            }
            combined.push_str(stderr.trim());
        }
        return Ok(combined);
    }

    Err(CommandCapture {
        stdout,
        stderr,
        code: output.status.code(),
    })
}

fn summarize_command_error(error: &CommandCapture) -> String {
    let mut detail = error.stderr.trim();
    if detail.is_empty() {
        detail = error.stdout.trim();
    }
    if detail.is_empty() {
        return match error.code {
            Some(code) => format!("Command exited with status {code}."),
            None => "Command failed before it could start.".to_string(),
        };
    }

    let final_line = detail
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(detail);
    match error.code {
        Some(code) => format!("{final_line} (exit {code})"),
        None => final_line.to_string(),
    }
}

#[tauri::command]
fn realtime_key_status() -> RealtimeKeyStatus {
    match std::env::var("OPENAI_API_KEY") {
        Ok(value) if !value.trim().is_empty() => RealtimeKeyStatus {
            configured: true,
            source: "OPENAI_API_KEY".to_string(),
        },
        _ => RealtimeKeyStatus {
            configured: false,
            source: "missing".to_string(),
        },
    }
}

#[tauri::command]
async fn create_realtime_call(sdp: String) -> Result<String, String> {
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| "OPENAI_API_KEY is not configured for the Tauri process.".to_string())?;
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return Err("OPENAI_API_KEY is empty.".to_string());
    }

    let session = serde_json::json!({
        "type": "realtime",
        "model": "gpt-realtime-2",
        "audio": {
            "output": {
                "voice": "marin"
            }
        },
        "instructions": "You are OS1 voice mode. Keep spoken replies short, warm, and useful. You are the front door to a personal Windows agent workspace."
    });

    let form = reqwest::multipart::Form::new()
        .text("sdp", sdp)
        .text("session", session.to_string());

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.openai.com/v1/realtime/calls")
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|error| format!("Realtime call setup failed: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Could not read Realtime response: {error}"))?;

    if status.is_success() {
        Ok(body)
    } else {
        Err(format!("OpenAI Realtime returned {status}: {body}"))
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            ask_hermes,
            check_hermes_runtime,
            configure_hermes_provider,
            create_hermes_profile,
            create_realtime_call,
            detect_hermes,
            import_codex_auth_to_wsl,
            install_hermes,
            list_hermes_profiles,
            repair_hermes,
            run_hermes_doctor,
            run_profile_command,
            realtime_key_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running OS1 Windows client");
}
