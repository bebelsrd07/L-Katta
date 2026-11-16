//! Manipuler l'ordinateur pour l'utilisateur : fenêtres (kdotool sous KDE, hyprctl, swaymsg), clavier (ydotool, lancé à
//! la demande, souris coupée), son (wpctl), lecteurs multimédias (MPRIS), luminosité.
//!
//! Garde-fous, appliqués par le code (un modèle qui désobéit ne les franchit pas) :
//! 1. Interrupteur général : `controle` dans les règles strictes (l'administrateur peut
//!    l'interdire pour tous les comptes) et le réglage `controle` (« demander », « confiance »,
//!    « non »).
//! 2. Trois niveaux : regarder (sans demander) ; réversible (activer, réduire, agrandir, son,
//!    lecteur, luminosité : demandé, sauf en « confiance ») ; risqué (fermer une fenêtre, taper,
//!    raccourcis : TOUJOURS demandé, avec la fenêtre et le texte exacts).
//! 3. La bonne fenêtre : avant de taper, il active la fenêtre visée et vérifie que c'est bien
//!    elle qui a le clavier ; jamais sa propre bulle.
//! 4. Fenêtres interdites (mots de passe, banques, mobile money, pages de connexion), touches
//!    interdites (Ctrl+Alt+Suppr…), jamais de secret tapé.
//! 5. 25 actions au plus par demande ; Échap dans la bulle arrête tout de suite.

use std::process::Stdio;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use crate::config::which;

/// Chaque demande porte un numéro ; « Arrêter » (bouton ou Échap) arrête la demande en cours,
/// et elle seule : une demande abandonnée qui tourne encore en arrière-plan ne peut plus rien
/// faire, même quand la suivante a commencé.
static DEMANDE: AtomicU64 = AtomicU64::new(0);
static ARRETEE: AtomicU64 = AtomicU64::new(0);
static COMPTE: AtomicUsize = AtomicUsize::new(0);
const MAX_ACTIONS: usize = 25;

/// Message d'erreur d'une demande arrêtée (reconnu par `est_arret`).
pub const ARRETE: &str = "⏹ arrêté";

thread_local! {
    /// Numéro de la demande traitée par ce fil (0 : la demande en cours).
    static MIENNE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Une nouvelle demande commence : compteur à zéro ; rend son numéro.
pub fn nouvelle_demande() -> u64 {
    COMPTE.store(0, Ordering::Relaxed);
    DEMANDE.fetch_add(1, Ordering::Relaxed) + 1
}

/// Le fil qui traite la demande `n` le dit au début de son travail.
pub fn adopter(n: u64) {
    MIENNE.with(|m| m.set(n));
}

/// Numéro de la demande de ce fil (ou de la demande en cours).
pub fn demande() -> u64 {
    let n = MIENNE.with(|m| m.get());
    if n == 0 { DEMANDE.load(Ordering::Relaxed) } else { n }
}

/// Arrête la demande en cours.
pub fn arreter() {
    ARRETEE.store(DEMANDE.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// La demande de ce fil (ou la demande en cours) est-elle arrêtée ?
pub fn arrete() -> bool {
    let n = MIENNE.with(|m| m.get());
    let n = if n == 0 { DEMANDE.load(Ordering::Relaxed) } else { n };
    n > 0 && ARRETEE.load(Ordering::Relaxed) >= n
}

/// Cette erreur vient-elle d'un arrêt demandé ?
pub fn est_arret(e: &str) -> bool {
    e.contains(ARRETE)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Niveau {
    Lecture,
    Reversible,
    Risque,
}

/// Une fenêtre vue par KWin.
#[derive(Debug, Clone, PartialEq)]
pub struct Fenetre {
    pub id: String,
    pub titre: String,
    pub classe: String,
}

#[cfg_attr(windows, allow(dead_code))]
fn kdotool(args: &[&str]) -> Result<String, String> {
    if !which("kdotool") {
        return Err(t!("kdotool n'est pas installé (paquet kdotool) : je ne peux pas manipuler les fenêtres.").into());
    }
    let o = crate::hote::commande("kdotool").args(args).stderr(Stdio::null()).output().map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Outil des fenêtres de ce bureau : kdotool (KDE), hyprctl (Hyprland), swaymsg (Sway).
#[cfg_attr(windows, allow(dead_code))]
fn outil() -> String {
    let o = crate::environnement::courant().fenetres;
    if o.is_empty() || o == "aucun" {
        // GNOME : l'extension de L Katta voit toutes les fenêtres (Xwayland n'en voit qu'une
        // partie).
        if crate::gnome::bureau_gnome() && crate::gnome::active() {
            return "gnome".into();
        }
        // Sans outil dédié : le protocole Wayland standard (ou les conventions X11), si le
        // bureau l'offre.
        if crate::toplevels::disponible() {
            return "wayland".into();
        }
    }
    o
}

#[cfg_attr(windows, allow(dead_code))]
fn depuis_wl(f: crate::toplevels::Fenetre) -> Fenetre {
    Fenetre { id: f.id.to_string(), titre: f.titre, classe: f.classe }
}

#[cfg_attr(windows, allow(dead_code))]
fn sortie_json(prog: &str, args: &[&str]) -> Result<serde_json::Value, String> {
    let o = crate::hote::commande(prog).args(args).stderr(Stdio::null()).output().map_err(|e| e.to_string())?;
    serde_json::from_slice(&o.stdout).map_err(|e| format!("{prog} : {e}"))
}

#[cfg_attr(windows, allow(dead_code))]
fn lancer(prog: &str, args: &[&str]) -> Result<(), String> {
    let o = crate::hote::commande(prog).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().map_err(|e| e.to_string())?;
    if o.success() { Ok(()) } else { Err(format!("{prog} a refusé")) }
}

/// Fenêtres de Sway (arbre des conteneurs) : (id, titre, application, active).
pub fn fenetres_sway(arbre: &serde_json::Value) -> Vec<(Fenetre, bool)> {
    let mut out = vec![];
    let mut pile = vec![arbre];
    while let Some(n) = pile.pop() {
        let fenetre = n.get("pid").is_some_and(|p| p.is_number()) && n["type"].as_str().is_some_and(|t| t == "con" || t == "floating_con");
        if fenetre {
            let classe = n["app_id"].as_str().filter(|s| !s.is_empty()).or_else(|| n["window_properties"]["class"].as_str()).unwrap_or("").to_string();
            out.push((Fenetre { id: n["id"].as_i64().unwrap_or(0).to_string(), titre: n["name"].as_str().unwrap_or("").to_string(), classe }, n["focused"].as_bool().unwrap_or(false)));
        }
        for k in ["nodes", "floating_nodes"] {
            if let Some(a) = n[k].as_array() {
                pile.extend(a.iter());
            }
        }
    }
    out
}

/// Fenêtres de Hyprland (`hyprctl clients -j`).
pub fn fenetres_hypr(v: &serde_json::Value) -> Vec<Fenetre> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["mapped"].as_bool().unwrap_or(true))
        .map(|c| Fenetre { id: c["address"].as_str().unwrap_or("").to_string(), titre: c["title"].as_str().unwrap_or("").to_string(), classe: c["class"].as_str().unwrap_or("").to_string() })
        .collect()
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use super::Fenetre;

    type Hwnd = isize;

    const GW_OWNER: u32 = 4;
    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_TOOLWINDOW: isize = 0x0000_0080;
    const WS_EX_NOACTIVATE: isize = 0x0800_0000;
    const SW_MINIMIZE: i32 = 6;
    const SW_MAXIMIZE: i32 = 3;
    const SW_RESTORE: i32 = 9;
    const WM_CLOSE: u32 = 0x0010;
    const INPUT_KEYBOARD: u32 = 1;
    const KEYEVENTF_KEYUP: u32 = 0x0002;
    const KEYEVENTF_UNICODE: u32 = 0x0004;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct KeybdInput {
        vk: u16,
        scan: u16,
        flags: u32,
        time: u32,
        extra: usize,
        // Même taille que MOUSEINPUT (la branche la plus large de l'union INPUT sur 64 bits).
        _pad: [u8; 8],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Input {
        kind: u32,
        ki: KeybdInput,
    }

    #[cfg(target_pointer_width = "64")]
    const _: () = assert!(std::mem::size_of::<Input>() == 40);

    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(cb: unsafe extern "system" fn(Hwnd, isize) -> i32, lparam: isize) -> i32;
        fn IsWindow(h: Hwnd) -> i32;
        fn IsWindowVisible(h: Hwnd) -> i32;
        fn IsIconic(h: Hwnd) -> i32;
        fn IsZoomed(h: Hwnd) -> i32;
        fn GetWindow(h: Hwnd, cmd: u32) -> Hwnd;
        fn GetWindowLongPtrW(h: Hwnd, idx: i32) -> isize;
        fn GetWindowTextW(h: Hwnd, buf: *mut u16, max: i32) -> i32;
        fn GetClassNameW(h: Hwnd, buf: *mut u16, max: i32) -> i32;
        fn GetForegroundWindow() -> Hwnd;
        fn SetForegroundWindow(h: Hwnd) -> i32;
        fn ShowWindow(h: Hwnd, cmd: i32) -> i32;
        fn PostMessageW(h: Hwnd, msg: u32, w: usize, l: isize) -> i32;
        fn SendInput(n: u32, inputs: *const Input, size: i32) -> u32;
    }

    fn lire(h: Hwnd, classe: bool) -> String {
        let mut buf = [0u16; 256];
        let n = unsafe {
            if classe { GetClassNameW(h, buf.as_mut_ptr(), buf.len() as i32) } else { GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32) }
        };
        if n <= 0 { String::new() } else { String::from_utf16_lossy(&buf[..n as usize]) }
    }

    fn ignorer_classe(c: &str) -> bool {
        matches!(
            c,
            "LkattaMaki" | "lkatta" | "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "NotifyIconOverflowWindow" | "Windows.UI.Core.CoreWindow"
        )
    }

    unsafe extern "system" fn collecter(h: Hwnd, lparam: isize) -> i32 {
        unsafe {
            if IsWindowVisible(h) == 0 || GetWindow(h, GW_OWNER) != 0 {
                return 1;
            }
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            if ex & (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) != 0 {
                return 1;
            }
            let classe = lire(h, true);
            let titre = lire(h, false);
            if ignorer_classe(&classe) || titre.trim().is_empty() {
                return 1;
            }
            let v = &mut *(lparam as *mut Vec<Fenetre>);
            v.push(Fenetre { id: h.to_string(), titre, classe });
            1
        }
    }

    pub fn fenetres() -> Vec<Fenetre> {
        let mut v: Vec<Fenetre> = vec![];
        // SAFETY : `v` vit pendant tout l'appel synchrone `EnumWindows`.
        unsafe { EnumWindows(collecter, (&mut v as *mut Vec<Fenetre>) as isize) };
        v
    }

    pub fn active() -> Option<Fenetre> {
        let h = unsafe { GetForegroundWindow() };
        if h == 0 {
            return None;
        }
        let classe = lire(h, true);
        let titre = lire(h, false);
        (!ignorer_classe(&classe) && !titre.is_empty()).then(|| Fenetre { id: h.to_string(), titre, classe })
    }

    fn hwnd(f: &Fenetre) -> Result<Hwnd, String> {
        let h: Hwnd = f.id.parse().map_err(|_| t!("Aucune fenêtre ouverte ne correspond à « {} ».", f.titre))?;
        if h == 0 || unsafe { IsWindow(h) } == 0 {
            return Err(t!("Aucune fenêtre ouverte ne correspond à « {} ».", f.titre));
        }
        let _ = 0 as *const c_void;
        Ok(h)
    }

    pub fn activer(f: &Fenetre) -> Result<(), String> {
        let h = hwnd(f)?;
        unsafe {
            if IsIconic(h) != 0 {
                ShowWindow(h, SW_RESTORE);
            }
            SetForegroundWindow(h);
        }
        Ok(())
    }

    pub fn action(f: &Fenetre, act: &str) -> Result<String, String> {
        let h = hwnd(f)?;
        match act {
            "activer" => activer(f).map(|_| t!("« {} » est au premier plan.", f.titre)),
            "reduire" => {
                unsafe { ShowWindow(h, SW_MINIMIZE) };
                Ok(t!("« {} » est réduite.", f.titre))
            }
            "agrandir" => {
                unsafe { ShowWindow(h, if IsZoomed(h) != 0 { SW_RESTORE } else { SW_MAXIMIZE }) };
                Ok(t!("« {} » : taille basculée (agrandie / normale).", f.titre))
            }
            "fermer" => {
                unsafe { PostMessageW(h, WM_CLOSE, 0, 0) };
                Ok(t!("« {} » : fermeture demandée (l'application peut demander d'enregistrer).", f.titre))
            }
            "bureau" => Err(t!("changer une fenêtre de bureau n'est pas possible sur ce bureau").into()),
            a => Err(t!("action de fenêtre inconnue : {}", a)),
        }
    }

    fn envoyer(inputs: &[Input]) -> Result<(), String> {
        if inputs.is_empty() {
            return Ok(());
        }
        // SAFETY : tableau d'INPUT valide, taille exacte passée à SendInput.
        let n = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<Input>() as i32) };
        if n == inputs.len() as u32 { Ok(()) } else { Err("SendInput refusé par Windows (fenêtre élevée ?)".into()) }
    }

    pub fn frapper_vk(vks: &[u16]) -> Result<(), String> {
        let mk = |vk: u16, flags: u32| Input { kind: INPUT_KEYBOARD, ki: KeybdInput { vk, scan: 0, flags, time: 0, extra: 0, _pad: [0; 8] } };
        let mut ins: Vec<Input> = vks.iter().map(|&k| mk(k, 0)).collect();
        ins.extend(vks.iter().rev().map(|&k| mk(k, KEYEVENTF_KEYUP)));
        envoyer(&ins)
    }

    pub fn taper_unicode(texte: &str) -> Result<(), String> {
        let mk = |scan: u16, flags: u32| Input { kind: INPUT_KEYBOARD, ki: KeybdInput { vk: 0, scan, flags: KEYEVENTF_UNICODE | flags, time: 0, extra: 0, _pad: [0; 8] } };
        let mut ins = Vec::with_capacity(texte.len() * 2);
        for c in texte.chars() {
            if c == '\n' {
                ins.push(Input { kind: INPUT_KEYBOARD, ki: KeybdInput { vk: 0x0D, scan: 0, flags: 0, time: 0, extra: 0, _pad: [0; 8] } });
                ins.push(Input { kind: INPUT_KEYBOARD, ki: KeybdInput { vk: 0x0D, scan: 0, flags: KEYEVENTF_KEYUP, time: 0, extra: 0, _pad: [0; 8] } });
                continue;
            }
            if c == '\r' {
                continue;
            }
            let mut u = [0u16; 2];
            for &code in c.encode_utf16(&mut u).iter() {
                ins.push(mk(code, 0));
                ins.push(mk(code, KEYEVENTF_KEYUP));
            }
        }
        envoyer(&ins)
    }

    pub fn vk(nom: &str) -> Option<u16> {
        let n = nom.trim().to_lowercase();
        if n.chars().count() == 1 {
            let c = n.chars().next()?;
            if c.is_ascii_lowercase() {
                return Some(c.to_ascii_uppercase() as u16);
            }
            if c.is_ascii_digit() {
                return Some(c as u16);
            }
        }
        Some(match n.as_str() {
            "ctrl" | "control" | "ctrl_l" => 0x11,
            "shift" | "maj" => 0x10,
            "alt" => 0x12,
            "altgr" => 0xA5,
            "super" | "meta" | "win" => 0x5B,
            "enter" | "entree" | "entrée" | "return" => 0x0D,
            "esc" | "escape" | "echap" | "échap" => 0x1B,
            "tab" => 0x09,
            "backspace" | "retour" => 0x08,
            "delete" | "del" | "suppr" => 0x2E,
            "space" | "espace" => 0x20,
            "up" | "haut" => 0x26,
            "down" | "bas" => 0x28,
            "left" | "gauche" => 0x25,
            "right" | "droite" => 0x27,
            "home" | "debut" | "début" => 0x24,
            "end" | "fin" => 0x23,
            "pageup" => 0x21,
            "pagedown" => 0x22,
            "print" | "impr" => 0x2C,
            "minus" | "-" => 0xBD,
            "plus" | "=" => 0xBB,
            f if f.starts_with('f') && f.len() <= 3 => match f[1..].parse::<u16>().ok()? {
                k @ 1..=24 => 0x70 + k - 1,
                _ => return None,
            },
            _ => return None,
        })
    }
}

/// Fenêtres ouvertes (sauf le maki lui-même et les panneaux du bureau).
pub fn fenetres() -> Result<Vec<Fenetre>, String> {
    #[cfg(windows)]
    return Ok(win::fenetres());
    #[cfg(not(windows))]
    {
    let garder = |f: &Fenetre| {
        let c = f.classe.to_lowercase();
        !(c.contains("lkatta") || c == "plasmashell" || c.contains("org.kde.plasmashell") || f.titre.trim().is_empty())
    };
    match outil().as_str() {
        "hyprctl" => return Ok(fenetres_hypr(&sortie_json("hyprctl", &["clients", "-j"])?).into_iter().filter(garder).collect()),
        "swaymsg" => return Ok(fenetres_sway(&sortie_json("swaymsg", &["-t", "get_tree"])?).into_iter().map(|(f, _)| f).filter(garder).collect()),
        "wayland" => return Ok(crate::toplevels::liste().into_iter().map(depuis_wl).filter(garder).collect()),
        "gnome" => return Ok(crate::gnome::fenetres()?.into_iter().map(depuis_wl).filter(garder).collect()),
        "kdotool" => {}
        _ if crate::gnome::bureau_gnome() => return Err(t!("l'extension GNOME de L Katta n'est pas active : tape /gnome pour l'installer").into()),
        _ if !which("kdotool") => return Err(t!("je ne sais pas encore piloter les fenêtres de ce bureau (KDE avec kdotool, Hyprland, Sway et les bureaux qui offrent le protocole Wayland standard sont gérés)").into()),
        _ => {}
    }
    let ids = kdotool(&["search", "--class", "."])?;
    let mut v = vec![];
    for id in ids.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let titre = kdotool(&["getwindowname", id]).unwrap_or_default();
        let classe = kdotool(&["getwindowclassname", id]).unwrap_or_default();
        let c = classe.to_lowercase();
        if c.contains("lkatta") || c == "plasmashell" || c.contains("org.kde.plasmashell") || titre.trim().is_empty() {
            continue;
        }
        v.push(Fenetre { id: id.to_string(), titre, classe });
    }
    Ok(v)
    }
}

pub fn active() -> Option<Fenetre> {
    #[cfg(windows)]
    return win::active();
    #[cfg(not(windows))]
    {
    match outil().as_str() {
        "hyprctl" => {
            let v = sortie_json("hyprctl", &["activewindow", "-j"]).ok()?;
            return fenetres_hypr(&serde_json::Value::Array(vec![v])).into_iter().next().filter(|f| !f.id.is_empty());
        }
        "swaymsg" => return fenetres_sway(&sortie_json("swaymsg", &["-t", "get_tree"]).ok()?).into_iter().find(|(_, a)| *a).map(|(f, _)| f),
        "wayland" => return crate::toplevels::active().map(depuis_wl),
        "gnome" => return crate::gnome::fenetres().ok()?.into_iter().find(|f| f.active).map(depuis_wl),
        _ => {}
    }
    let id = kdotool(&["getactivewindow"]).ok().filter(|s| !s.is_empty())?;
    Some(Fenetre { titre: kdotool(&["getwindowname", &id]).unwrap_or_default(), classe: kdotool(&["getwindowclassname", &id]).unwrap_or_default(), id })
    }
}

/// La fenêtre dont le titre ou l'application contient `nom` (la plus courte d'abord : la
/// plus précise).
pub fn trouver(nom: &str) -> Result<Fenetre, String> {
    let n = crate::semantique::simplifier(nom.trim());
    if n.is_empty() {
        return Err(t!("Quelle fenêtre ? (un mot de son titre ou le nom de l'application)").into());
    }
    let mut l: Vec<Fenetre> = fenetres()?.into_iter().filter(|f| crate::semantique::simplifier(&format!("{} {}", f.titre, f.classe)).contains(&n)).collect();
    l.sort_by_key(|f| f.titre.len());
    l.into_iter().next().ok_or_else(|| t!("Aucune fenêtre ouverte ne correspond à « {} ».", nom))
}

/// Fenêtre où il ne faut jamais taper ni envoyer de touches (règles strictes).
pub fn interdite(f: &Fenetre, regles: &crate::regles::Regles) -> Option<String> {
    let t = crate::semantique::simplifier(&format!("{} {}", f.titre, f.classe));
    regles
        .fenetres_interdites
        .iter()
        .find(|m| !m.trim().is_empty() && t.contains(&crate::semantique::simplifier(m)))
        .map(|m| t!("la fenêtre « {} » est protégée par les règles strictes ({})", f.titre, m))
}

/// Combinaisons jamais envoyées (session, redémarrage, consoles).
pub fn touches_interdites(combo: &str, regles: &crate::regles::Regles) -> bool {
    let c = combo.to_lowercase().replace(' ', "");
    let base = ["ctrl+alt+delete", "ctrl+alt+del", "ctrl+alt+suppr", "ctrl+alt+backspace", "alt+sysrq", "alt+print+", "ctrl+alt+f"];
    base.iter().any(|b| c.starts_with(b)) || regles.touches_interdites.iter().any(|t| !t.is_empty() && c == t.to_lowercase().replace(' ', ""))
}

/// Code de touche Linux (clavier QWERTY américain, la disposition des combinaisons).
fn code(nom: &str) -> Option<u16> {
    let n = nom.trim().to_lowercase();
    let lettres = "qwertyuiop";
    let rang2 = "asdfghjkl";
    let rang3 = "zxcvbnm";
    if n.chars().count() == 1 {
        let c = n.chars().next()?;
        if let Some(i) = lettres.find(c) {
            return Some(16 + i as u16);
        }
        if let Some(i) = rang2.find(c) {
            return Some(30 + i as u16);
        }
        if let Some(i) = rang3.find(c) {
            return Some(44 + i as u16);
        }
        if let Some(d) = c.to_digit(10) {
            return Some(if d == 0 { 11 } else { 1 + d as u16 });
        }
    }
    Some(match n.as_str() {
        "ctrl" | "control" | "ctrl_l" => 29,
        "shift" | "maj" => 42,
        "alt" => 56,
        "altgr" => 100,
        "super" | "meta" | "win" => 125,
        "enter" | "entree" | "entrée" | "return" => 28,
        "esc" | "escape" | "echap" | "échap" => 1,
        "tab" => 15,
        "backspace" | "retour" => 14,
        "delete" | "del" | "suppr" => 111,
        "space" | "espace" => 57,
        "up" | "haut" => 103,
        "down" | "bas" => 108,
        "left" | "gauche" => 105,
        "right" | "droite" => 106,
        "home" | "debut" | "début" => 102,
        "end" | "fin" => 107,
        "pageup" => 104,
        "pagedown" => 109,
        "print" | "impr" => 99,
        "minus" | "-" => 12,
        "plus" | "=" => 13,
        f if f.starts_with('f') && f.len() <= 3 => match f[1..].parse::<u16>().ok()? {
            k @ 1..=10 => 58 + k,
            11 => 87,
            12 => 88,
            _ => return None,
        },
        _ => return None,
    })
}

/// « ctrl+shift+t » → séquence ydotool (appuis puis relâchements dans l'ordre inverse).
pub fn sequence(combo: &str) -> Result<Vec<String>, String> {
    let codes: Vec<u16> = combo.split('+').map(|k| code(k).ok_or_else(|| t!("touche inconnue : « {} »", k.trim()))).collect::<Result<_, String>>()?;
    if codes.is_empty() {
        return Err(t!("aucune touche").into());
    }
    let mut v: Vec<String> = codes.iter().map(|c| format!("{c}:1")).collect();
    v.extend(codes.iter().rev().map(|c| format!("{c}:0")));
    Ok(v)
}

/// Prise du démon ydotool lancé par L Katta (clavier seulement, souris coupée).
#[cfg_attr(windows, allow(dead_code))]
fn prise() -> std::path::PathBuf {
    crate::paths::runtime_dir().join("ydotool.sock")
}

#[cfg_attr(windows, allow(dead_code))]
fn ydotool(args: &[String]) -> Result<(), String> {
    if !which("ydotool") || !which("ydotoold") {
        return Err(t!("ydotool n'est pas installé (paquet ydotool) : je ne peux pas taper au clavier.").into());
    }
    let p = prise();
    if !p.exists() {
        crate::paths::mourir_avec_parent(&mut crate::hote::commande("ydotoold"))
            .arg(format!("--socket-path={}", p.display()))
            .arg("--mouse-off")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(100));
            if p.exists() {
                break;
            }
        }
    }
    let ok = crate::hote::commande_env("ydotool", &[("YDOTOOL_SOCKET", p.as_os_str())]).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
    if ok { Ok(()) } else { Err(t!("ydotool n'a pas pu envoyer les touches (accès à /dev/uinput ?).").into()) }
}

/// Active la fenêtre et vérifie qu'elle a bien le clavier.
fn viser(f: &Fenetre) -> Result<(), String> {
    activer(f)?;
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(80));
        if active().is_some_and(|a| a.id == f.id) {
            return Ok(());
        }
    }
    Err(t!("La fenêtre « {} » n'a pas pris le clavier : je n'envoie rien.", f.titre))
}

/// Tape un texte dans une fenêtre : collé (toutes les lettres, accents compris, quelle que
/// soit la disposition du clavier), puis le presse-papiers d'avant est remis.
pub fn taper(f: &Fenetre, texte: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        viser(f)?;
        win::taper_unicode(texte)?;
        return Ok(t!("Texte tapé dans « {} » ({} caractères).", f.titre, texte.chars().count()));
    }
    #[cfg(not(windows))]
    {
    viser(f)?;
    let avant = crate::clipboard::coller();
    crate::clipboard::copy(texte).map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(120));
    let terminal = ["konsole", "terminal", "kitty", "alacritty", "wezterm", "foot", "yakuake"].iter().any(|t| f.classe.to_lowercase().contains(t));
    let combo = if terminal { "ctrl+shift+v" } else { "ctrl+v" };
    let r = ydotool(&std::iter::once("key".to_string()).chain(sequence(combo)?).collect::<Vec<_>>());
    std::thread::sleep(Duration::from_millis(250));
    if let Some(b) = avant {
        let _ = crate::clipboard::copy(&String::from_utf8_lossy(&b));
    }
    r.map(|_| t!("Texte tapé dans « {} » ({} caractères).", f.titre, texte.chars().count()))
    }
}

pub fn touches(f: &Fenetre, combo: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        let vks: Vec<u16> = combo.split('+').map(|k| win::vk(k).ok_or_else(|| t!("touche inconnue : « {} »", k.trim()))).collect::<Result<_, String>>()?;
        if vks.is_empty() {
            return Err(t!("aucune touche").into());
        }
        viser(f)?;
        return win::frapper_vk(&vks).map(|_| t!("« {} » envoyé à « {} ».", combo, f.titre));
    }
    #[cfg(not(windows))]
    {
        let seq = sequence(combo)?;
        viser(f)?;
        ydotool(&std::iter::once("key".to_string()).chain(seq).collect::<Vec<_>>()).map(|_| t!("« {} » envoyé à « {} ».", combo, f.titre))
    }
}

/// Met une fenêtre au premier plan, avec l'outil de ce bureau.
fn activer(f: &Fenetre) -> Result<(), String> {
    #[cfg(windows)]
    return win::activer(f);
    #[cfg(not(windows))]
    match outil().as_str() {
        "hyprctl" => lancer("hyprctl", &["dispatch", "focuswindow", &format!("address:{}", f.id)]),
        "swaymsg" => lancer("swaymsg", &[&format!("[con_id={}]", f.id), "focus"]),
        "wayland" => {
            crate::toplevels::demander(f.id.parse().unwrap_or(0), crate::toplevels::Action::Activer);
            Ok(())
        }
        "gnome" => crate::gnome::action(f.id.parse().unwrap_or(0), "activer"),
        _ => kdotool(&["windowactivate", &f.id]).map(|_| ()),
    }
}

/// Action sur une fenêtre : activer, reduire, agrandir, fermer, bureau (n).
pub fn fenetre(f: &Fenetre, action: &str, bureau: Option<u32>) -> Result<String, String> {
    #[cfg(windows)]
    {
        let _ = bureau;
        return win::action(f, action);
    }
    #[cfg(not(windows))]
    {
    let o = outil();
    if o == "gnome" {
        let id: u32 = f.id.parse().unwrap_or(0);
        let m = match action {
            "activer" => t!("« {} » est au premier plan.", f.titre),
            "reduire" => t!("« {} » est réduite.", f.titre),
            "agrandir" => t!("« {} » : taille basculée (agrandie / normale).", f.titre),
            "fermer" => t!("« {} » : fermeture demandée (l'application peut demander d'enregistrer).", f.titre),
            "bureau" => return Err(t!("changer une fenêtre de bureau n'est pas possible sur ce bureau").into()),
            a => return Err(t!("action de fenêtre inconnue : {}", a)),
        };
        return crate::gnome::action(id, action).map(|_| m);
    }
    if o == "wayland" {
        use crate::toplevels::Action;
        let id: u32 = f.id.parse().unwrap_or(0);
        let (a, m) = match action {
            "activer" => (Action::Activer, t!("« {} » est au premier plan.", f.titre)),
            "reduire" => (Action::Reduire, t!("« {} » est réduite.", f.titre)),
            "agrandir" => (Action::Agrandir, t!("« {} » : taille basculée (agrandie / normale).", f.titre)),
            "fermer" => (Action::Fermer, t!("« {} » : fermeture demandée (l'application peut demander d'enregistrer).", f.titre)),
            "bureau" => return Err(t!("changer une fenêtre de bureau n'est pas possible sur ce bureau").into()),
            a => return Err(t!("action de fenêtre inconnue : {}", a)),
        };
        crate::toplevels::demander(id, a);
        return Ok(m);
    }
    if o == "hyprctl" || o == "swaymsg" {
        let fait = |r: Result<(), String>, m: String| r.map(|_| m);
        let (sel_h, sel_s) = (format!("address:{}", f.id), format!("[con_id={}]", f.id));
        return match (o.as_str(), action) {
            (_, "activer") => fait(activer(f), t!("« {} » est au premier plan.", f.titre)),
            ("hyprctl", "fermer") => fait(lancer("hyprctl", &["dispatch", "closewindow", &sel_h]), t!("« {} » : fermeture demandée (l'application peut demander d'enregistrer).", f.titre)),
            ("swaymsg", "fermer") => fait(lancer("swaymsg", &[&sel_s, "kill"]), t!("« {} » : fermeture demandée (l'application peut demander d'enregistrer).", f.titre)),
            ("hyprctl", "reduire") => fait(lancer("hyprctl", &["dispatch", "movetoworkspacesilent", &format!("special:minimized,{sel_h}")]), t!("« {} » est réduite.", f.titre)),
            ("swaymsg", "reduire") => fait(lancer("swaymsg", &[&sel_s, "move", "scratchpad"]), t!("« {} » est réduite.", f.titre)),
            ("hyprctl", "agrandir") => fait(activer(f).and_then(|_| lancer("hyprctl", &["dispatch", "fullscreen", "1"])), t!("« {} » : taille basculée (agrandie / normale).", f.titre)),
            ("swaymsg", "agrandir") => fait(lancer("swaymsg", &[&sel_s, "fullscreen", "toggle"]), t!("« {} » : taille basculée (agrandie / normale).", f.titre)),
            (_, "bureau") => {
                let n = bureau.ok_or(t!("quel bureau ? (numéro)"))?;
                let r = if o == "hyprctl" { lancer("hyprctl", &["dispatch", "movetoworkspacesilent", &format!("{n},{sel_h}")]) } else { lancer("swaymsg", &[&sel_s, "move", "container", "to", "workspace", "number", &n.to_string()]) };
                fait(r, t!("« {} » est sur le bureau {}.", f.titre, n))
            }
            (_, a) => Err(t!("action de fenêtre inconnue : {}", a)),
        };
    }
    match action {
        "activer" => kdotool(&["windowactivate", &f.id]).map(|_| t!("« {} » est au premier plan.", f.titre)),
        "reduire" => kdotool(&["windowminimize", &f.id]).map(|_| t!("« {} » est réduite.", f.titre)),
        "agrandir" => kdotool(&["windowstate", "--toggle", "maximized", &f.id]).map(|_| t!("« {} » : taille basculée (agrandie / normale).", f.titre)),
        "fermer" => kdotool(&["windowclose", &f.id]).map(|_| t!("« {} » : fermeture demandée (l'application peut demander d'enregistrer).", f.titre)),
        "bureau" => {
            let n = bureau.ok_or(t!("quel bureau ? (numéro)"))?;
            kdotool(&["set_desktop_for_window", &f.id, &n.to_string()]).map(|_| t!("« {} » est sur le bureau {}.", f.titre, n))
        }
        a => Err(t!("action de fenêtre inconnue : {}", a)),
    }
    }
}

/// Son : volume (0–150 %), plus, moins, muet.
pub fn son(action: &str, valeur: Option<u32>) -> Result<String, String> {
    #[cfg(windows)]
    {
        // Touches multimédias Windows (VK_VOLUME_MUTE = 0xAD, DOWN = 0xAE, UP = 0xAF).
        return match action {
            "plus" => (0..5).try_for_each(|_| win::frapper_vk(&[0xAF])).map(|_| t!("Volume monté.").into()),
            "moins" => (0..5).try_for_each(|_| win::frapper_vk(&[0xAE])).map(|_| t!("Volume baissé.").into()),
            "muet" => win::frapper_vk(&[0xAD]).map(|_| t!("Son coupé ou rétabli.").into()),
            "volume" => {
                let v = valeur.ok_or(t!("quel volume ? (0 à 150)"))?.min(100);
                // Remet à zéro puis monte par pas de 2 %.
                for _ in 0..50 {
                    win::frapper_vk(&[0xAE])?;
                }
                for _ in 0..(v.div_ceil(2)) {
                    win::frapper_vk(&[0xAF])?;
                }
                Ok(t!("Volume à {} %.", v))
            }
            a => Err(t!("action de son inconnue : {}", a)),
        };
    }
    #[cfg(not(windows))]
    {
    if !which("wpctl") {
        return Err(t!("wpctl (PipeWire) n'est pas installé.").into());
    }
    let cible = "@DEFAULT_AUDIO_SINK@";
    let (args, msg): (Vec<String>, String) = match action {
        "volume" => {
            let v = valeur.ok_or(t!("quel volume ? (0 à 150)"))?.min(150);
            (vec!["set-volume".into(), cible.into(), format!("{v}%")], t!("Volume à {} %.", v))
        }
        "plus" => (vec!["set-volume".into(), "-l".into(), "1.5".into(), cible.into(), "10%+".into()], t!("Volume monté.").into()),
        "moins" => (vec!["set-volume".into(), cible.into(), "10%-".into()], t!("Volume baissé.").into()),
        "muet" => (vec!["set-mute".into(), cible.into(), "toggle".into()], t!("Son coupé ou rétabli.").into()),
        a => return Err(t!("action de son inconnue : {}", a)),
    };
    let ok = crate::hote::commande("wpctl").args(&args).status().map(|s| s.success()).unwrap_or(false);
    if ok { Ok(msg) } else { Err(t!("wpctl a échoué.").into()) }
    }
}

/// Lecteur multimédia (MPRIS sous Linux, touches multimédias sous Windows) : lecture, pause, suivant, precedent.
pub fn lecteur(action: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        // VK_MEDIA_NEXT_TRACK = 0xB0, PREV = 0xB1, PLAY_PAUSE = 0xB3.
        let vk = match action {
            "lecture" | "pause" | "lecture_pause" => 0xB3,
            "suivant" => 0xB0,
            "precedent" | "précédent" => 0xB1,
            a => return Err(t!("action de lecteur inconnue : {}", a)),
        };
        return win::frapper_vk(&[vk]).map(|_| t!("Lecteur : {}.", action));
    }
    #[cfg(not(windows))]
    {
    let methode = match action {
        "lecture" | "pause" | "lecture_pause" => "PlayPause",
        "suivant" => "Next",
        "precedent" | "précédent" => "Previous",
        a => return Err(t!("action de lecteur inconnue : {}", a)),
    };
    let noms = crate::hote::commande("qdbus6").stderr(Stdio::null()).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let lecteur = noms.lines().map(str::trim).find(|l| l.starts_with("org.mpris.MediaPlayer2.")).ok_or(t!("Aucun lecteur multimédia ouvert."))?.to_string();
    let ok = crate::hote::commande("qdbus6").args([&lecteur, "/org/mpris/MediaPlayer2", &format!("org.mpris.MediaPlayer2.Player.{methode}")]).status().map(|s| s.success()).unwrap_or(false);
    if ok { Ok(t!("Lecteur : {}.", action)) } else { Err(t!("Le lecteur n'a pas répondu.").into()) }
    }
}

/// Luminosité de l'écran (0–100 %), par le service d'énergie de KDE.
#[cfg(windows)]
pub fn luminosite(_pourcent: u32) -> Result<String, String> {
    Err(t!("Luminosité non réglable ici.").into())
}

#[cfg(not(windows))]
pub fn luminosite(pourcent: u32) -> Result<String, String> {
    let svc = "org.kde.Solid.PowerManagement";
    let chemin = "/org/kde/Solid/PowerManagement/Actions/BrightnessControl";
    let max: u32 = crate::hote::commande("qdbus6").args([svc, chemin, "brightnessMax"]).output().ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok()).ok_or(t!("Luminosité non réglable ici."))?;
    let v = (max * pourcent.min(100) / 100).max(1);
    let ok = crate::hote::commande("qdbus6").args([svc, chemin, "setBrightness", &v.to_string()]).status().map(|s| s.success()).unwrap_or(false);
    if ok { Ok(t!("Luminosité à {} %.", pourcent.min(100))) } else { Err(t!("Le réglage de luminosité a échoué.").into()) }
}

/// Vérifie qu'une action est permise maintenant (interrupteurs, arrêt, quota par demande).
pub fn permis(regles: &crate::regles::Regles, cfg: &crate::config::Config, niveau: Niveau) -> Result<(), String> {
    if !regles.controle {
        return Err(t!("Le contrôle de l'ordinateur est interdit par les règles strictes de cette machine.").into());
    }
    if cfg.controle == "non" && niveau != Niveau::Lecture {
        return Err(t!("Le contrôle de l'ordinateur est coupé dans les réglages (Assistant › Contrôle de l'ordinateur).").into());
    }
    if arrete() {
        return Err(t!("Arrêté par l'utilisateur (Échap) : plus aucune action pour cette demande.").into());
    }
    if niveau != Niveau::Lecture && COMPTE.fetch_add(1, Ordering::Relaxed) >= MAX_ACTIONS {
        return Err(t!("Limite de {} actions par demande atteinte : je m'arrête là.", MAX_ACTIONS));
    }
    Ok(())
}

/// Faut-il demander l'accord ? (Toujours pour le risqué ; pour le réversible, sauf en confiance.)
pub fn demander(cfg: &crate::config::Config, niveau: Niveau) -> bool {
    match niveau {
        Niveau::Lecture => false,
        Niveau::Reversible => cfg.controle != "confiance",
        Niveau::Risque => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_hyprland_and_sway_windows() {
        let h: serde_json::Value = serde_json::from_str(r#"[{"address":"0x5a1","title":"Budget — Vivaldi","class":"vivaldi-stable","mapped":true},{"address":"0x5a2","title":"","class":"x","mapped":false}]"#).unwrap();
        let l = fenetres_hypr(&h);
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].id.as_str(), l[0].classe.as_str()), ("0x5a1", "vivaldi-stable"));
        let s: serde_json::Value = serde_json::from_str(r#"{"type":"root","nodes":[{"type":"output","nodes":[{"type":"workspace","nodes":[{"type":"con","id":12,"pid":40,"name":"notes.md - Kate","app_id":"org.kde.kate","focused":true,"nodes":[]}],"floating_nodes":[{"type":"floating_con","id":13,"pid":41,"name":"Firefox","app_id":null,"window_properties":{"class":"firefox"},"focused":false,"nodes":[]}]}]}]}"#).unwrap();
        let l = fenetres_sway(&s);
        assert_eq!(l.len(), 2);
        let active = l.iter().find(|(_, a)| *a).unwrap();
        assert_eq!((active.0.id.as_str(), active.0.classe.as_str()), ("12", "org.kde.kate"));
        assert!(l.iter().any(|(f, _)| f.classe == "firefox"));
    }

    #[test]
    fn keys_and_guards() {
        assert_eq!(sequence("ctrl+s").unwrap(), vec!["29:1", "31:1", "31:0", "29:0"]);
        assert_eq!(sequence("Ctrl + Shift + T").unwrap(), vec!["29:1", "42:1", "20:1", "20:0", "42:0", "29:0"]);
        assert_eq!(sequence("Entrée").unwrap(), vec!["28:1", "28:0"]);
        assert_eq!(sequence("f5").unwrap(), vec!["63:1", "63:0"]);
        assert!(sequence("ctrl+€").is_err());
        let r = crate::regles::Regles::default();
        assert!(touches_interdites("Ctrl+Alt+Delete", &r));
        assert!(touches_interdites("ctrl+alt+f2", &r));
        assert!(!touches_interdites("ctrl+s", &r));
        let f = |t: &str, c: &str| Fenetre { id: "x".into(), titre: t.into(), classe: c.into() };
        assert!(interdite(&f("Coffre — KeePassXC", "org.keepassxc.KeePassXC"), &r).is_some());
        assert!(interdite(&f("BNI Madagascar — Connexion - Firefox", "firefox"), &r).is_some());
        assert!(interdite(&f("Document sans titre — LibreOffice Writer", "libreoffice-writer"), &r).is_none());
        nouvelle_demande();
        let mut cfg = crate::config::Config::default();
        assert!(permis(&r, &cfg, Niveau::Risque).is_ok());
        arreter();
        assert!(permis(&r, &cfg, Niveau::Reversible).is_err());
        nouvelle_demande();
        cfg.controle = "non".into();
        assert!(permis(&r, &cfg, Niveau::Reversible).is_err() && permis(&r, &cfg, Niveau::Lecture).is_ok());
        cfg.controle = "confiance".into();
        assert!(!demander(&cfg, Niveau::Reversible) && demander(&cfg, Niveau::Risque));
    }
}

#[cfg(test)]
mod essai_reel {
    /// Lecture seule : `cargo test liste_les_fenetres -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn liste_les_fenetres() {
        let l = super::fenetres().unwrap();
        for f in &l {
            println!("{} | {}", f.titre, f.classe);
        }
        println!("active : {:?}", super::active().map(|f| f.titre));
        assert!(!l.is_empty());
    }
}
