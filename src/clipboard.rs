//! Historique du presse-papiers.
//!
//! Sous Linux la lecture passe par `wl-paste --watch` (wl-clipboard) et la copie par `wl-copy`,
//! `xclip` ou `xsel`. Sous Windows le presse-papiers est une API et non un programme externe :
//! c'est le module `win` en bas de ce fichier, qui parle à `user32` et `kernel32`. L'historique
//! lui-même — `load`, `save`, `push` — est le même des deux côtés.
//!
//! Les copies marquées secrètes par les gestionnaires de mots de passe ne sont jamais
//! enregistrées : `x-kde-passwordManagerHint` sous Linux, le format
//! `ExcludeClipboardContentFromMonitorProcessing` sous Windows.

#[cfg(not(windows))]
use std::io::{BufRead, BufReader};
#[cfg(not(windows))]
use std::process::Stdio;

#[cfg(not(windows))]
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entree {
    pub texte: String,
    /// Horodatage RFC 3339.
    pub quand: String,
}

pub fn load() -> Vec<Entree> {
    std::fs::read_to_string(paths::clipboard_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(list: &[Entree]) -> std::io::Result<()> {
    let s = serde_json::to_string(list).map_err(std::io::Error::other)?;
    paths::write_private(&paths::clipboard_file(), s.as_bytes())
}

/// Ajoute une entrée en tête (sans doublon), borne la taille.
pub fn push(list: &mut Vec<Entree>, texte: String, max: usize) {
    if texte.trim().is_empty() {
        return;
    }
    list.retain(|e| e.texte != texte);
    list.insert(0, Entree { texte, quand: chrono::Local::now().to_rfc3339() });
    list.truncate(max.max(1));
}

/// Session Wayland avec wl-clipboard ; sinon X11 (xclip ou xsel).
#[cfg(not(windows))]
fn wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() && crate::config::which("wl-copy")
}

/// Place du texte dans le presse-papiers.
pub fn copy(texte: &str) -> std::io::Result<()> {
    #[cfg(windows)]
    return win::copier(texte);
    #[cfg(not(windows))]
    return copier_programmes(texte);
}

/// `wl-copy`, sinon `xclip` ou `xsel` sous X11.
#[cfg(not(windows))]
fn copier_programmes(texte: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut cmd = if wayland() {
        crate::hote::commande("wl-copy")
    } else if crate::config::which("xclip") {
        let mut c = crate::hote::commande("xclip");
        c.args(["-selection", "clipboard"]);
        c
    } else {
        let mut c = crate::hote::commande("xsel");
        c.arg("-ib");
        c
    };
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    child.stdin.take().expect("stdin").write_all(texte.as_bytes())?;
    child.wait()?;
    Ok(())
}

/// Texte du presse-papiers, en octets UTF-8 (aucun texte : `None`).
pub fn coller() -> Option<Vec<u8>> {
    #[cfg(windows)]
    return win::lire().map(|(t, _)| t.into_bytes());
    #[cfg(not(windows))]
    return coller_programmes();
}

/// `wl-paste`, sinon `xclip` ou `xsel` sous X11.
#[cfg(not(windows))]
fn coller_programmes() -> Option<Vec<u8>> {
    let mut cmd = if wayland() {
        let mut c = crate::hote::commande("wl-paste");
        c.args(["--no-newline", "--type", "text"]);
        c
    } else if crate::config::which("xclip") {
        let mut c = crate::hote::commande("xclip");
        c.args(["-selection", "clipboard", "-o"]);
        c
    } else {
        let mut c = crate::hote::commande("xsel");
        c.arg("-ob");
        c
    };
    cmd.stderr(Stdio::null()).output().ok().filter(|o| o.status.success()).map(|o| o.stdout)
}

#[cfg(not(windows))]
const WATCH_SCRIPT: &str = "if wl-paste -l 2>/dev/null | grep -q passwordManagerHint; then cat >/dev/null; echo SECRET; \
                            else base64 -w0; echo; fi";

/// Surveille le presse-papiers (bloquant : à lancer dans un fil). Appelle `on_new`
/// pour chaque nouvelle copie texte.
///
/// Sous Linux la surveillance est poussée par `wl-paste --watch` ; sous Windows rien ne pousse,
/// donc le module `win` interroge le numéro de séquence du presse-papiers, qui change à chaque
/// copie. Le fil ne revient jamais dans les deux cas : il meurt avec le processus.
pub fn watch(max: usize, mut on_new: impl FnMut(&str)) {
    #[cfg(windows)]
    return win::surveiller(max, &mut on_new);
    #[cfg(not(windows))]
    return surveiller_programmes(max, &mut on_new);
}

#[cfg(not(windows))]
fn surveiller_programmes(max: usize, on_new: &mut impl FnMut(&str)) {
    if !crate::config::which("wl-paste") {
        eprintln!("{}", t!("lkatta : wl-paste absent, historique du presse-papiers désactivé"));
        return;
    }
    // Meurt avec le maki : sinon une relance laisse un surveillant orphelin.
    let child = crate::paths::mourir_avec_parent(&mut crate::hote::commande("wl-paste"))
        .args(["--type", "text", "--watch", "sh", "-c", WATCH_SCRIPT])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return };
    let out = child.stdout.take().expect("stdout");
    for line in BufReader::new(out).lines() {
        let Ok(line) = line else { break };
        if line == "SECRET" || line.is_empty() {
            continue;
        }
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(line.trim()) else { continue };
        let texte = String::from_utf8_lossy(&bytes).into_owned();
        if texte.len() > 20_000 {
            continue; // trop gros pour un historique léger
        }
        // Recharger : le panneau a pu vider l'historique.
        let mut list = load();
        push(&mut list, texte.clone(), max);
        let _ = save(&list);
        on_new(&texte);
    }
    let _ = child.wait();
}

/// Le presse-papiers de Windows : une API, pas un programme externe.
///
/// Sous Linux cette moitié passe par `wl-copy`, `wl-paste`, `xclip` ou `xsel`, qui n'existent pas
/// ici. `user32` et `kernel32` font tout le reste, et ce sont des bibliothèques du système :
/// aucune dépendance nouvelle (voir `docs/PLAN.md` §10).
///
/// Trois règles ne se lisent dans aucune signature et sont pourtant celles qui comptent :
///
/// - `EmptyClipboard` avant `SetClipboardData`, sinon le contenu précédent reste attribué à la
///   fenêtre qui l'avait posé ;
/// - après un `SetClipboardData` **réussi**, le bloc alloué appartient au système : le libérer
///   laisserait le presse-papiers pointer sur de la mémoire rendue ;
/// - `IsClipboardFormatAvailable` exige un presse-papiers ouvert, donc la détection du contenu
///   marqué secret et la lecture du texte se font dans la même ouverture.
#[cfg(windows)]
mod win {
    use std::io;

    /// Texte dans le presse-papiers : UTF-16 terminé par un NUL.
    const CF_UNICODETEXT: u32 = 13;
    const GMEM_MOVEABLE: u32 = 0x0002;
    /// Le format par lequel une application demande que sa copie ne soit pas enregistrée — le
    /// pendant Windows de `x-kde-passwordManagerHint`, que le script de surveillance Linux
    /// cherche sous Linux.
    const FORMAT_SECRET: &str = "ExcludeClipboardContentFromMonitorProcessing";
    /// Windows ne pousse rien à un fil qui n'a pas de fenêtre, donc il faut sonder. 400 ms est
    /// assez court pour que l'historique paraisse immédiat et assez long pour ne pas réveiller le
    /// processeur inutilement.
    const SONDE: std::time::Duration = std::time::Duration::from_millis(400);
    /// Même plafond que le chemin Linux : un historique léger, pas une copie du disque.
    const MAX_OCTETS: usize = 20_000;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn OpenClipboard(fenetre: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(format: u32, donnees: isize) -> isize;
        fn GetClipboardData(format: u32) -> isize;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
        fn RegisterClipboardFormatW(nom: *const u16) -> u32;
        fn GetClipboardSequenceNumber() -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalAlloc(drapeaux: u32, octets: usize) -> isize;
        fn GlobalLock(bloc: isize) -> *mut u16;
        fn GlobalUnlock(bloc: isize) -> i32;
        fn GlobalFree(bloc: isize) -> isize;
        fn GlobalSize(bloc: isize) -> usize;
    }

    /// Ouvre le presse-papiers, en réessayant un peu : une autre application peut le tenir.
    ///
    /// `OpenClipboard(0)` associe l'ouverture à la tâche courante plutôt qu'à une fenêtre. C'est
    /// ce qu'il faut ici : une copie peut venir d'un fil qui n'a aucune fenêtre, et refuser de
    /// copier pour cette raison serait pire que le risque.
    fn ouvrir() -> bool {
        for _ in 0..5 {
            // SAFETY : NULL est une valeur acceptée par l'API ; rien à libérer si l'appel échoue.
            if unsafe { OpenClipboard(0) } != 0 {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(40));
        }
        false
    }

    /// Texte du presse-papiers, et vrai si l'application qui l'a posé a demandé qu'il ne soit pas
    /// surveillé. `None` s'il n'y a pas de texte ou si le presse-papiers est tenu ailleurs.
    pub fn lire() -> Option<(String, bool)> {
        if !ouvrir() {
            return None;
        }
        let trouve = lire_ouvert();
        // SAFETY : le presse-papiers a été ouvert par `ouvrir` juste au-dessus. Le fermer est
        // obligatoire même en cas d'échec, sinon plus aucune autre application ne peut l'ouvrir.
        unsafe { CloseClipboard() };
        trouve
    }

    /// Le corps de `lire`, presse-papiers déjà ouvert.
    fn lire_ouvert() -> Option<(String, bool)> {
        // SAFETY : nom en UTF-16 terminé par un NUL, vivant jusqu'à la fin de l'appel.
        let secret = {
            let mut nom: Vec<u16> = FORMAT_SECRET.encode_utf16().collect();
            nom.push(0);
            unsafe { RegisterClipboardFormatW(nom.as_ptr()) }
        };
        let marque = secret != 0 && unsafe { IsClipboardFormatAvailable(secret) } != 0;
        if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) } == 0 {
            return None;
        }
        let bloc = unsafe { GetClipboardData(CF_UNICODETEXT) };
        if bloc == 0 {
            return None;
        }
        let ptr = unsafe { GlobalLock(bloc) };
        if ptr.is_null() {
            return None;
        }
        // `GlobalSize` borne la lecture : sans elle, un bloc qui ne se termine pas par un NUL
        // ferait chercher ce zéro au-delà du bloc, dans la mémoire du processus.
        let max = unsafe { GlobalSize(bloc) } / std::mem::size_of::<u16>();
        let texte = unsafe {
            let mut n = 0;
            while n < max && *ptr.add(n) != 0 {
                n += 1;
            }
            // SAFETY : `n` caractères valides dans le bloc verrouillé, bornés par sa taille.
            String::from_utf16_lossy(std::slice::from_raw_parts(ptr, n))
        };
        // SAFETY : verrouillé ci-dessus et pas encore déverrouillé.
        unsafe { GlobalUnlock(bloc) };
        Some((texte, marque))
    }

    /// Place du texte dans le presse-papiers.
    pub fn copier(texte: &str) -> io::Result<()> {
        if !ouvrir() {
            return Err(io::Error::other(t!("le presse-papiers est tenu par une autre application")));
        }
        let fait = poser(texte);
        // SAFETY : ouvert par `ouvrir` ci-dessus, quoi que `poser` ait fait entre-temps.
        unsafe { CloseClipboard() };
        fait
    }

    /// Le corps de `copier`, presse-papiers déjà ouvert.
    fn poser(texte: &str) -> io::Result<()> {
        // SAFETY : le presse-papiers est ouvert, c'est la condition que pose l'API.
        if unsafe { EmptyClipboard() } == 0 {
            return Err(io::Error::other("EmptyClipboard"));
        }
        let mut u16s: Vec<u16> = texte.encode_utf16().collect();
        u16s.push(0); // CF_UNICODETEXT est du texte terminé par un NUL.
        let bloc = unsafe { GlobalAlloc(GMEM_MOVEABLE, u16s.len() * std::mem::size_of::<u16>()) };
        if bloc == 0 {
            return Err(io::Error::other("GlobalAlloc"));
        }
        let ptr = unsafe { GlobalLock(bloc) };
        if ptr.is_null() {
            // SAFETY : l'appel a échoué, donc le bloc est encore à nous.
            unsafe { GlobalFree(bloc) };
            return Err(io::Error::other("GlobalLock"));
        }
        // SAFETY : `GlobalAlloc` vient de rendre exactement `u16s.len()` mots, les deux zones ne
        // se recouvrent pas (l'une vient d'être allouée, l'autre est une `Vec` locale).
        unsafe { std::ptr::copy_nonoverlapping(u16s.as_ptr(), ptr, u16s.len()) };
        // SAFETY : verrouillé juste au-dessus.
        unsafe { GlobalUnlock(bloc) };
        // Réussi, le bloc appartient désormais au système : surtout ne pas le libérer ici.
        if unsafe { SetClipboardData(CF_UNICODETEXT, bloc) } == 0 {
            // SAFETY : l'appel a échoué, donc le bloc est encore à nous.
            unsafe { GlobalFree(bloc) };
            return Err(io::Error::other("SetClipboardData"));
        }
        Ok(())
    }

    /// Surveille le presse-papiers : ne revient jamais, comme le `wl-paste --watch` de Linux.
    pub fn surveiller(max: usize, on_new: &mut impl FnMut(&str)) {
        // SAFETY : lecture d'un compteur du système, rien à ouvrir ni à libérer.
        let mut vu = unsafe { GetClipboardSequenceNumber() };
        loop {
            std::thread::sleep(SONDE);
            // SAFETY : idem.
            let maintenant = unsafe { GetClipboardSequenceNumber() };
            if maintenant == vu {
                continue;
            }
            vu = maintenant;
            let Some((texte, marque)) = lire() else { continue };
            // Marqué secret par un gestionnaire de mots de passe : jamais enregistré.
            if marque || texte.trim().is_empty() {
                continue;
            }
            if texte.len() > MAX_OCTETS {
                continue; // trop gros pour un historique léger
            }
            // Relire à chaque copie : le panneau a pu vider l'historique entre-temps.
            let mut list = super::load();
            super::push(&mut list, texte.clone(), max);
            let _ = super::save(&list);
            on_new(&texte);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_dedup_and_truncate() {
        let mut l = vec![];
        push(&mut l, "a".into(), 2);
        push(&mut l, "b".into(), 2);
        push(&mut l, "a".into(), 2);
        assert_eq!(l.iter().map(|e| e.texte.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        push(&mut l, "c".into(), 2);
        assert_eq!(l.len(), 2);
        push(&mut l, "  ".into(), 2);
        assert_eq!(l[0].texte, "c");
    }
}
