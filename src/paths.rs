//! Emplacements des fichiers de L Katta (XDG).

use std::path::PathBuf;

#[cfg(not(windows))]
fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

#[cfg(windows)]
fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| {
            let (d, p) = (std::env::var_os("HOMEDRIVE")?, std::env::var_os("HOMEPATH")?);
            let mut r = std::ffi::OsString::from(d);
            r.push(p);
            Some(PathBuf::from(r))
        })
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(not(windows))]
fn xdg(var: &str, fallback: &str) -> PathBuf {
    // Dans le Flatpak : les dossiers de la machine (~/.config/lkatta…), les mêmes que ceux de
    // l'AppImage ; ceux du bac à sable (~/.var/app/…) ne sont pas vus par KWin ni GNOME Shell.
    let var = if std::path::Path::new("/.flatpak-info").exists() { format!("HOST_{var}") } else { var.to_string() };
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
}

#[cfg(windows)]
fn dossier_win(surcharge_xdg: &str, var_win: &str, repli: &str) -> PathBuf {
    // `XDG_*` n'est jamais posé par Windows, mais sert aux essais isolés et aux utilisateurs qui
    // choisissent expressément un autre emplacement. Sinon `%APPDATA%` et `%LOCALAPPDATA%`.
    std::env::var_os(surcharge_xdg)
        .or_else(|| std::env::var_os(var_win))
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(repli))
}

/// `~/.config/lkatta` sous Linux, `%APPDATA%\lkatta` sous Windows.
pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    return dossier_win("XDG_CONFIG_HOME", "APPDATA", r"AppData\Roaming").join("lkatta");
    #[cfg(not(windows))]
    xdg("XDG_CONFIG_HOME", ".config").join("lkatta")
}

/// `~/.local/share/lkatta` sous Linux, `%LOCALAPPDATA%\lkatta` sous Windows.
pub fn data_dir() -> PathBuf {
    #[cfg(windows)]
    return dossier_win("XDG_DATA_HOME", "LOCALAPPDATA", r"AppData\Local").join("lkatta");
    #[cfg(not(windows))]
    xdg("XDG_DATA_HOME", ".local/share").join("lkatta")
}

/// Emplacement d'installation utilisateur (`~/.local/bin/lkatta` ou
/// `%LOCALAPPDATA%\Programs\LKatta\lkatta.exe`).
pub fn binaire_installe() -> PathBuf {
    #[cfg(windows)]
    return dossier_win("LKATTA_INSTALL_DIR", "LOCALAPPDATA", r"AppData\Local").join(r"Programs\LKatta\lkatta.exe");
    #[cfg(not(windows))]
    home().join(".local/bin/lkatta")
}

/// Dossier d'exécution (socket, verrous). Repli sur le dossier de données.
pub fn runtime_dir() -> PathBuf {
    // Flatpak : seul $XDG_RUNTIME_DIR/app/<id> est commun aux instances (maki et `flatpak run … dire`).
    let flatpak = std::env::var("FLATPAK_ID").ok().filter(|_| std::path::Path::new("/.flatpak-info").exists());
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(|p| match &flatpak {
            Some(id) => PathBuf::from(p).join("app").join(id).join("lkatta"),
            None => PathBuf::from(p).join("lkatta"),
        })
        .unwrap_or_else(|| data_dir().join("run"))
}

/// Dossiers de L Katta lisibles par l'utilisateur seul (discussions, index, captures,
/// presse-papiers : rien pour les autres comptes de la machine).
pub fn fermer_dossiers() {
    // Les droits eux-mêmes sont dans le socle : 0700/0600 sur Linux, et sur Windows l'ACL
    // héritée de %LOCALAPPDATA% (voir src/os/mod.rs).
    for d in [config_dir(), data_dir(), data_dir().join("captures"), data_dir().join("fils")] {
        let _ = std::fs::create_dir_all(&d);
        let _ = crate::os::restreindre_dossier(&d);
    }
    for f in ["moteur.log", "relance.log", "garde-memoire.log"] {
        let _ = crate::os::restreindre_fichier(&data_dir().join(f));
    }
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}
pub fn prompt_file() -> PathBuf {
    config_dir().join("prompt.md")
}
pub fn machine_file() -> PathBuf {
    config_dir().join("machine.md")
}
pub fn reminders_file() -> PathBuf {
    data_dir().join("rappels.json")
}
pub fn clipboard_file() -> PathBuf {
    data_dir().join("presse-papiers.json")
}
#[cfg_attr(windows, allow(dead_code))]
pub fn socket_file() -> PathBuf {
    runtime_dir().join("lkatta.sock")
}

pub fn home_dir() -> PathBuf {
    home()
}

/// Fait mourir le processus lancé avec L Katta (moniteurs `gdbus monitor` : sinon ils restent
/// orphelins à chaque relance). Relais vers le socle, gardé ici parce que c'est `paths` qui
/// sait déjà où se trouve le programme à relancer.
pub fn mourir_avec_parent(cmd: &mut std::process::Command) -> &mut std::process::Command {
    crate::os::mourir_avec_parent(cmd)
}

/// Chemin du programme à relancer : l'AppImage si on tourne depuis elle ; sinon le binaire
/// courant (sans le suffixe « (deleted) » que Linux ajoute quand le fichier a été remplacé
/// pendant qu'il tournait, par une mise à jour ou une recompilation), ou à défaut celui
/// installé dans ~/.local/bin.
pub fn self_exe() -> PathBuf {
    if let Some(a) = std::env::var_os("APPIMAGE") {
        return PathBuf::from(a);
    }
    let installed = binaire_installe();
    match std::env::current_exe() {
        Ok(p) => {
            let s = p.to_string_lossy();
            let clean = PathBuf::from(s.strip_suffix(" (deleted)").unwrap_or(&s));
            if clean.exists() { clean } else { installed }
        }
        Err(_) => installed,
    }
}

thread_local! {
    /// Dossier de travail du fil qui tourne (côté Code : le projet).
    static TRAVAIL: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Fixe le dossier de travail de ce fil d'exécution (`None` : dossier personnel).
pub fn set_travail(d: Option<PathBuf>) {
    TRAVAIL.with(|t| *t.borrow_mut() = d.filter(|p| p.is_dir()));
}

/// Dossier du projet de ce fil (côté Code), s'il y en a un.
pub fn projet() -> Option<PathBuf> {
    TRAVAIL.with(|t| t.borrow().clone())
}

/// Dossier d'où partent les commandes et les chemins relatifs.
pub fn travail() -> PathBuf {
    TRAVAIL.with(|t| t.borrow().clone()).unwrap_or_else(home)
}

/// Remplace un `~` initial par le dossier personnel ; un chemin relatif part du dossier de
/// travail (le projet, côté Code).
pub fn expand(p: &str) -> PathBuf {
    if p == "~" {
        home()
    } else if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix(r"~\")) {
        // « ~//x » et « ~\\x » restent sous le dossier personnel (join remplacerait tout par la racine).
        home().join(rest.trim_start_matches(['/', '\\']))
    } else if !p.is_empty() && !std::path::Path::new(p).is_absolute() {
        travail().join(p)
    } else {
        PathBuf::from(p)
    }
}

/// Écrit un fichier de façon atomique, droits 600 (données personnelles).
pub fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Nom temporaire propre à ce fichier et à cet appel (« regles.toml » et « regles.md » ne
    // partagent plus « regles.tmp ») ; créé neuf pour que les droits 600 s'appliquent toujours.
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nom = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{nom}.{}-{}.tmp", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let _ = std::fs::remove_file(&tmp);
    {
        // `options_privees()` : le temporaire naît déjà privé, il n'y a pas de fenêtre où un
        // autre compte pourrait lire ce qui vient d'être écrit.
        let mut f = crate::os::options_privees().write(true).create_new(true).open(&tmp)?;
        if let Err(e) = f.write_all(contents) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    }
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_handles_tilde_and_absolute_paths() {
        assert_eq!(expand("~"), home_dir());
        assert_eq!(expand("~/Documents/note.md"), home_dir().join("Documents/note.md"));
        assert_eq!(expand(r"~\Documents"), home_dir().join("Documents"));
        assert_eq!(expand("~//secret"), home_dir().join("secret"));
        #[cfg(windows)]
        {
            assert!(config_dir().is_absolute());
            assert!(data_dir().is_absolute());
            assert_eq!(expand(r"C:\Projets\demo"), PathBuf::from(r"C:\Projets\demo"));
        }
    }
}
