//! Programmes de la machine (gdbus, wl-copy, terminal, gestionnaire de paquets…).
//!
//! Hors Flatpak : lancés directement. Dans le Flatpak (`/.flatpak-info`), le bac à sable ne
//! les contient pas : ils passent par `flatpak-spawn --host`, qui les lance sur la machine
//! avec les droits de l'utilisateur (docs/SECURITE.md : ce Flatpak n'est pas un bac à sable).
//! Les variables d'environnement ne traversent pas : `commande_env` les passe en `--env=`.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

pub const APP_ID: &str = "io.github.raantss18.LKatta";

/// Tourne-t-on dans le Flatpak ?
pub fn flatpak() -> bool {
    static F: OnceLock<bool> = OnceLock::new();
    *F.get_or_init(|| std::path::Path::new("/.flatpak-info").exists())
}

/// Commande lancée sur la machine.
pub fn commande(prog: impl AsRef<OsStr>) -> Command {
    commande_env(prog, &[])
}

/// Commande lancée sur la machine, avec des variables d'environnement en plus.
pub fn commande_env(prog: impl AsRef<OsStr>, env: &[(&str, &OsStr)]) -> Command {
    if !flatpak() {
        let mut c = Command::new(prog);
        for (k, v) in env {
            c.env(k, v);
        }
        return c;
    }
    let mut c = Command::new("flatpak-spawn");
    c.arg("--host");
    for (k, v) in env {
        let mut a = std::ffi::OsString::from(format!("--env={k}="));
        a.push(v);
        c.arg(a);
    }
    c.arg(prog);
    c
}

/// Le programme existe-t-il sur la machine (dans son PATH) ?
pub fn existe(prog: &str) -> bool {
    if !flatpak() {
        return dans_path(prog);
    }
    static VUS: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);
    if let Some(v) = VUS.lock().ok().and_then(|m| m.as_ref().and_then(|m| m.get(prog).copied())) {
        return v;
    }
    let v = Command::new("flatpak-spawn")
        .args(["--host", "sh", "-c", "command -v \"$1\" >/dev/null", "sh", prog])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if let Ok(mut m) = VUS.lock() {
        m.get_or_insert_with(HashMap::new).insert(prog.to_string(), v);
    }
    v
}

fn dans_path(prog: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(prog).is_file()))
        .unwrap_or(false)
}

/// Ligne de shell (sur la machine) qui relance L Katta.
pub fn relancer_lkatta() -> String {
    if flatpak() {
        format!("exec setsid flatpak run {APP_ID}")
    } else {
        format!("APPIMAGELAUNCHER_DISABLE=1 exec setsid '{}'", crate::paths::self_exe().display())
    }
}

/// Commande (pour un fichier .desktop, un raccourci) qui lance L Katta depuis le bureau.
pub fn lanceur_bureau() -> String {
    if flatpak() {
        format!("flatpak run {APP_ID}")
    } else {
        format!("env APPIMAGELAUNCHER_DISABLE=1 {}", crate::install::lanceur().display())
    }
}

/// Installé par un gestionnaire de paquets (pacman, Flatpak…) : c'est lui qui met à jour.
pub fn gestionnaire() -> Option<String> {
    if flatpak() {
        return Some("Flatpak".into());
    }
    if std::env::var_os("APPIMAGE").is_some() {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    (exe.starts_with("/usr/") || exe.starts_with("/opt/")).then(|| t!("ton gestionnaire de paquets").into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn host_commands_outside_flatpak() {
        if super::flatpak() {
            return;
        }
        let c = super::commande_env("true", &[("A", std::ffi::OsStr::new("1"))]);
        assert_eq!(c.get_program(), "true");
        assert!(c.get_envs().any(|(k, v)| k == "A" && v == Some(std::ffi::OsStr::new("1"))));
        assert!(super::existe("sh"));
        assert!(super::lanceur_bureau().contains("APPIMAGELAUNCHER_DISABLE=1"));
    }
}
