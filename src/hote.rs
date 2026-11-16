//! Programmes de la machine (gdbus, wl-copy, terminal, gestionnaire de paquets…).
//!
//! Hors Flatpak : lancés directement. Dans le Flatpak (`/.flatpak-info`), le bac à sable ne
//! les contient pas : ils passent par `flatpak-spawn --host`, qui les lance sur la machine
//! avec les droits de l'utilisateur (docs/SECURITE.md : ce Flatpak n'est pas un bac à sable).
//! Les variables d'environnement ne traversent pas : `commande_env` les passe en `--env=`.
//!
//! Sous Windows il n'y a ni `xdg-open` ni Flatpak : `ouvrir` passe par `ShellExecuteW`, et
//! `commande` lance le programme demandé tel quel. Rien d'autre ici ne dépend de Linux.

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

/// Fichier de /etc de la machine : dans le Flatpak, /etc est celui du bac à sable ; celui de
/// la machine est dans /run/host/etc (accès host-etc) et os-release dans /run/host.
pub fn etc(chemin: &str) -> String {
    if !flatpak() {
        return chemin.to_string();
    }
    match chemin {
        "/etc/os-release" => "/run/host/os-release".to_string(),
        c => c.strip_prefix("/etc/").map(|r| format!("/run/host/etc/{r}")).unwrap_or_else(|| c.to_string()),
    }
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

/// Ouvre un fichier, un dossier ou une adresse web avec l'application par défaut.
///
/// Prend un `OsStr` et non une `String` : un chemin n'est pas toujours de l'UTF-8 valide, et
/// `to_string_lossy` ferait ouvrir un autre fichier que celui demandé.
///
/// Ouvre, et n'exécute jamais — voir `a_refuser`.
#[cfg(unix)]
pub fn ouvrir(cible: &OsStr) -> std::io::Result<()> {
    // `commande` et non `Command::new` : dans le Flatpak, `xdg-open` doit passer par
    // `flatpak-spawn --host` pour atteindre le navigateur de la machine.
    commande("xdg-open").arg(cible).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    Ok(())
}

#[cfg(windows)]
pub fn ouvrir(cible: &OsStr) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    if a_refuser(cible) {
        return Err(std::io::Error::other("ouvrir montre un document, il n'exécute pas de programme"));
    }
    // Deux chaînes UTF-16 terminées par un NUL, comme toute l'API `W`. `encode_wide` et non une
    // conversion depuis `to_string_lossy` : un chemin non UTF-8 resterait le même chemin.
    let operation: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
    let cible: Vec<u16> = cible.encode_wide().chain(std::iter::once(0)).collect();
    const SW_SHOWNORMAL: i32 = 1;
    // SAFETY : les deux chaînes vivent jusqu'à la fin de l'appel et se terminent par un NUL ;
    // paramètres et dossier à NULL, ce que l'API accepte ; fenêtre à NULL, rien ici n'étant
    // modal. `ShellExecuteW` et non `cmd /c start` : faire traverser une adresse par un
    // interpréteur de commandes transformerait un « & » ou un « | » en commande.
    let r = unsafe { ShellExecuteW(0, operation.as_ptr(), cible.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
    // En dessous de 32 ce n'est pas une poignée mais un code d'échec. Le test de signe vient
    // d'abord : `as usize` d'une valeur négative donnerait un très grand nombre, donc un échec
    // pris pour une réussite.
    if r < 0 || (r as usize) < 32 {
        return Err(std::io::Error::other(format!("ShellExecuteW a refusé d'ouvrir (code {r})")));
    }
    Ok(())
}

/// Ce que `ouvrir` refuse sous Windows : les programmes et les scripts.
///
/// `xdg-open` consulte la base mime, et un script s'ouvre dans un éditeur. `ShellExecuteW` avec
/// « open » fait autre chose : il **exécute** un `.exe`, un `.bat` ou un `.ps1`. Or `ouvrir` est
/// atteignable par l'outil `ouvrir` que le modèle peut appeler avec la cible de son choix — sans
/// cette garde, cet outil deviendrait sous Windows un moyen de lancer n'importe quel programme.
///
/// La garde est donc du côté où est le danger : sous Linux, `xdg-open` garde exactement son
/// comportement d'aujourd'hui.
#[cfg_attr(not(windows), allow(dead_code))]
fn a_refuser(cible: &OsStr) -> bool {
    const DANGEREUX: [&str; 12] = ["exe", "bat", "cmd", "com", "scr", "msi", "ps1", "vbs", "js", "jse", "hta", "wsf"];
    let nom = cible.to_string_lossy();
    // Une adresse web se reconnaît à son schéma : « https://x/outil.exe » doit rester ouvrable,
    // c'est le navigateur qui décide ce qu'il en fait, et il ne l'exécute pas.
    if nom.contains("://") {
        return false;
    }
    let Some(point) = nom.rfind('.') else { return false };
    DANGEREUX.contains(&nom[point + 1..].to_ascii_lowercase().as_str())
}

#[cfg(windows)]
#[link(name = "shell32")]
unsafe extern "system" {
    /// Ouvre un document ou une adresse avec l'application associée. Rend une pseudo-poignée :
    /// au-dessus de 32 cela a marché, en dessous c'est un code d'échec.
    fn ShellExecuteW(
        fenetre: isize,
        operation: *const u16,
        cible: *const u16,
        parametres: *const u16,
        dossier: *const u16,
        commande: i32,
    ) -> isize;
}

#[cfg(test)]
mod tests {
    /// La garde de `ouvrir` : ce qu'elle refuse, et ce qu'elle laisse passer.
    #[test]
    fn open_never_runs_a_program() {
        use std::ffi::OsStr;
        for c in ["outil.exe", "C:\\outils\\outil.EXE", "install.msi", "script.ps1", "macro.vbs", "départ.bat", "page.hta", "tâche.wsf", "charge.js"] {
            assert!(super::a_refuser(OsStr::new(c)), "{c} devrait être refusé");
        }
        for c in ["notes.md", "config.toml", "photo.png", "facture.pdf", "C:\\docs\\rapport.docx", "/home/moi/Documents", "sans-extension", "https://exemple.fr/outil.exe", "http://a.b/c.ps1"] {
            assert!(!super::a_refuser(OsStr::new(c)), "{c} devrait passer");
        }
    }

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
        assert_eq!(super::etc("/etc/os-release"), "/etc/os-release");
    }
}
