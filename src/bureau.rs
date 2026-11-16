//! Ce qui se passe sur le bureau, pour rester discret :
//! - un petit script KWin signale la fenêtre active (plein écran, position, application,
//!   titre, écran) au service D-Bus `org.lkatta.Maki` de L Katta ;
//! - une veille légère regarde « Ne pas déranger » (Plasma) et l'usage du micro (visio).

use std::process::Stdio;
use std::time::Duration;

use crate::config::which;
use crate::ipc::Msg;
use crate::paths;

/// Fenêtre active, telle que KWin la décrit (coordonnées globales, px logiques).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fenetre {
    pub plein_ecran: bool,
    pub rect: (f32, f32, f32, f32),
    pub classe: String,
    pub titre: String,
    pub ecran: String,
    /// Taille de la scène de GNOME (px logiques), pour convertir sous Xwayland mis à
    /// l'échelle ; (0, 0) : coordonnées déjà dans celles du maki.
    pub scene: (f32, f32),
}

impl Fenetre {
    #[cfg_attr(windows, allow(dead_code))]
    pub fn from_json(s: &str) -> Option<Fenetre> {
        let v: serde_json::Value = serde_json::from_str(s).ok()?;
        let n = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
        let t = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        Some(Fenetre {
            plein_ecran: v.get("plein").and_then(|x| x.as_bool()).unwrap_or(false),
            rect: (n("x"), n("y"), n("w"), n("h")),
            classe: t("classe"),
            titre: t("titre"),
            ecran: t("ecran"),
            scene: (n("sw"), n("sh")),
        })
    }

    /// Une visio ou une présentation, d'après l'application ou le titre.
    pub fn visio(&self) -> bool {
        let c = format!("{} {}", self.classe, self.titre).to_lowercase();
        ["zoom", "teams", "meet -", "google meet", "jitsi", "skype", "webex", "discord", "obs", "présentation", "presentation", "impress", "slideshow", "diaporama"]
            .iter()
            .any(|k| c.contains(k))
    }
}

/// Une fenêtre couvre-t-elle tout le moniteur ?
///
/// Les deux rectangles sont `(gauche, haut, droite, bas)`, dans le même repère. Une fenêtre
/// **maximisée** s'arrête à la barre des tâches et ne couvre donc pas le moniteur entier : c'est
/// toute la différence, et elle compte — comparer au mauvais rectangle ferait croire à L Katta que
/// l'utilisateur est en plein écran dès qu'il agrandit une fenêtre, et le réduirait au silence pour
/// un oui pour un non.
///
/// C'est du Rust pur et non un appel système pour que la règle soit testée sur toutes les cibles,
/// y compris là où aucun moniteur ne peut la renseigner. Le `allow` n'est là que pour la CI Linux,
/// qui compile avec `-D warnings` : hors Windows, seuls les essais l'appellent.
#[cfg_attr(not(windows), allow(dead_code))]
fn couvre_tout(fenetre: (i32, i32, i32, i32), ecran: (i32, i32, i32, i32)) -> bool {
    fenetre.0 <= ecran.0 && fenetre.1 <= ecran.1 && fenetre.2 >= ecran.2 && fenetre.3 >= ecran.3
}

#[cfg_attr(windows, allow(dead_code))]
const SCRIPT: &str = r#"// L Katta : signale la fenêtre active pour rester discret (chargé et retiré par L Katta).
function signaler() {
    var w = workspace.activeWindow;
    // Le maki lui-même, les panneaux, les notifications : on garde la fenêtre précédente.
    if (w && (String(w.resourceClass) === "lkatta" || w.dock || w.notification || w.criticalNotification || w.onScreenDisplay)) return;
    // Le bureau vide : rien à éviter.
    if (w && w.desktopWindow) w = null;
    var g = w ? w.frameGeometry : null;
    callDBus("org.lkatta.Maki", "/Maki", "org.lkatta.Maki", "Fenetre", JSON.stringify({
        plein: w ? !!w.fullScreen : false,
        x: g ? g.x : 0, y: g ? g.y : 0, w: g ? g.width : 0, h: g ? g.height : 0,
        classe: w ? String(w.resourceClass) : "", titre: w ? String(w.caption) : "",
        ecran: (w && w.output) ? String(w.output.name) : ""}));
}
var suivies = [];
workspace.windowActivated.connect(function (w) {
    if (w && suivies.indexOf(w) < 0) {
        suivies.push(w);
        w.frameGeometryChanged.connect(signaler);
        w.fullScreenChanged.connect(signaler);
    }
    signaler();
});
signaler();
"#;

#[cfg_attr(windows, allow(dead_code))]
fn qdbus(args: &[&str]) -> Option<String> {
    let bin = if which("qdbus6") { "qdbus6" } else { "qdbus" };
    let o = crate::hote::commande(bin).args(args).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Charge (ou recharge) le script KWin.
#[cfg_attr(windows, allow(dead_code))]
pub fn charger_script() {
    let path = paths::data_dir().join("kwin-lkatta.js");
    if paths::write_private(&path, SCRIPT.as_bytes()).is_err() {
        return;
    }
    let p = path.to_string_lossy().into_owned();
    if qdbus(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.isScriptLoaded", "lkatta"]).as_deref() == Some("true") {
        let _ = qdbus(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.unloadScript", "lkatta"]);
    }
    let _ = qdbus(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.loadScript", &p, "lkatta"]);
    let _ = qdbus(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.start"]);
}

pub fn retirer_script() {
    #[cfg(not(windows))]
    let _ = qdbus(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.unloadScript", "lkatta"]);
}

/// Le service D-Bus que le script KWin appelle pour dire quelle fenêtre est active.
/// Linux seulement : `zbus` n'est plus une dépendance que pour cette cible.
#[cfg(target_os = "linux")]
struct Service {
    tx: Box<dyn Fn(Msg) + Send + Sync>,
}

/// Un pont du bureau (script KWin, extension GNOME) a envoyé la fenêtre active : il voit
/// toutes les fenêtres, mieux que les conventions X11 (qui sous Xwayland ne voient que les
/// fenêtres X11).
static PONT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg_attr(windows, allow(dead_code))]
pub fn pont_actif() -> bool {
    PONT.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(target_os = "linux")]
#[zbus::interface(name = "org.lkatta.Maki")]
impl Service {
    /// La fenêtre active, en JSON (KWin envoie les nombres entiers en `int` : un seul texte
    /// évite toute surprise de signature).
    #[zbus(name = "Fenetre")]
    fn fenetre(&self, json: String) {
        if let Some(f) = Fenetre::from_json(&json) {
            PONT.store(true, std::sync::atomic::Ordering::Relaxed);
            (self.tx)(Msg::Fenetre(f));
        }
    }
}

/// Démarre le service D-Bus (fil dédié) puis charge le script KWin.
#[cfg(target_os = "linux")]
pub fn demarrer(tx: impl Fn(Msg) + Send + Sync + 'static) {
    std::thread::spawn(move || {
        let r: zbus::Result<()> = zbus::block_on(async move {
            // Sans « remplacer » : un deuxième L Katta (essai, autre affichage) ne doit pas
            // prendre le nom au maki en marche, qui ne verrait plus la fenêtre active.
            let _conn = zbus::connection::Builder::session()
                .replace_existing_names(false)
                .name("org.lkatta.Maki")
                .serve_at("/Maki", Service { tx: Box::new(tx) })
                .build()
                .await?;
            charger_script();
            std::future::pending::<()>().await;
            Ok(())
        });
        if let Err(e) = r {
            eprintln!("lkatta : service D-Bus indisponible ({e}) ; discrétion réduite");
        }
    });
}

/// Démarre la veille de la fenêtre active.
///
/// Sous Linux c'est le bureau qui pousse l'information (script KWin, extension GNOME) par D-Bus.
/// Windows ne pousse rien à un fil qui n'a pas de fenêtre : la fenêtre de devant se demande au
/// système — `GetForegroundWindow`, puis `GetWindowTextW`, `GetClassNameW` et `GetWindowRect`,
/// toutes dans `user32`. Aucune dépendance nouvelle, comme le reste du backend Win32.
///
/// C'est donc un sondage et non une écoute. Toutes les 400 ms, ce fil compare à ce qu'il avait et
/// n'envoie `Msg::Fenetre` qu'au changement : renvoyer la même fenêtre trois fois par seconde
/// réveillerait la boucle du maki pour rien. 400 ms, c'est assez court pour que le maki descende
/// d'une fenêtre qu'on vient de fermer sans que ça se voie comme un retard, et assez long pour que
/// le fil dorme l'essentiel du temps.
///
/// L'alternative exacte est `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)`, une vraie notification
/// sans sondage. Elle exige une boucle de messages dans le fil qui pose le hook — donc un fil de
/// plus avec sa propre boucle — pour un renseignement qui change quelques fois par minute. Le
/// sondage est retenu tant que 400 ms suffisent ; le choix est écrit ici pour rester visible.
///
/// Sur les autres systèmes (un macOS éventuel) rien n'est lancé et `pont_actif` reste faux : la
/// discrétion de L Katta ne s'y applique pas. Renvoyer une fenêtre vide ferait croire que
/// l'utilisateur ne fait rien, donc parler quand il faut se taire.
#[cfg(not(target_os = "linux"))]
pub fn demarrer(tx: impl Fn(Msg) + Send + Sync + 'static) {
    #[cfg(windows)]
    return win::demarrer(tx);
    #[cfg(not(windows))]
    let _ = tx;
}

/// La fenêtre de devant, demandée à Windows.
#[cfg(windows)]
mod win {
    use std::time::Duration;

    use crate::bureau::Fenetre;
    use crate::ipc::Msg;

    /// Le pas du sondage. Voir la doc de `demarrer` pour pourquoi 400 ms et pas une notification.
    const PAS: Duration = Duration::from_millis(400);
    /// Notre propre fenêtre, dont la classe est fixée dans `mascot_win/win32.rs`. Elle est
    /// `NOACTIVATE`, donc elle ne passe normalement pas au premier plan — mais la reconnaître
    /// coûte une comparaison, et évite que L Katta se cache de lui-même le jour où une de ses
    /// fenêtres prend le clavier (la bulle de discussion le fera).
    const NOTRE_CLASSE: &str = "LkattaMaki";
    /// Un titre de fenêtre tient largement là-dedans ; au-delà, `GetWindowTextW` tronque.
    const TAILLE_TEXTE: usize = 1024;
    /// Le moniteur le plus proche, même si la fenêtre déborde ou est hors écran.
    const MONITOR_DEFAULTTONEAREST: u32 = 2;

    /// `RECT` du SDK : quatre entiers signés, dans cet ordre.
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    /// `MONITORINFO` du SDK. `rc_monitor` est le moniteur entier, `rc_work` le moniteur moins la
    /// barre des tâches : c'est la différence des deux qui distingue plein écran et maximisé.
    ///
    /// Nous ne lisons que `rc_monitor`. Les autres champs sont là parce que la structure est un
    /// contrat : `GetMonitorInfoW` les écrit tous, et retirer `rc_work` décalerait `dw_flags` et
    /// ferait lire n'importe quoi. D'où le `allow` — ce n'est pas du code mort, c'est de la place
    /// réservée par l'API.
    #[allow(dead_code)]
    #[repr(C)]
    struct InfoMoniteur {
        cb_size: u32,
        rc_monitor: Rect,
        rc_work: Rect,
        dw_flags: u32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> isize;
        fn GetWindowTextW(fenetre: isize, texte: *mut u16, max: i32) -> i32;
        fn GetClassNameW(fenetre: isize, nom: *mut u16, max: i32) -> i32;
        fn GetWindowRect(fenetre: isize, r: *mut Rect) -> i32;
        fn IsIconic(fenetre: isize) -> i32;
        fn MonitorFromWindow(fenetre: isize, drapeaux: u32) -> isize;
        fn GetMonitorInfoW(moniteur: isize, info: *mut std::ffi::c_void) -> i32;
    }

    /// Démarre le fil de sondage. Comme le service D-Bus de Linux, il ne revient pas.
    pub fn demarrer(tx: impl Fn(Msg) + Send + Sync + 'static) {
        std::thread::spawn(move || {
            let mut vu: Option<Fenetre> = None;
            loop {
                std::thread::sleep(PAS);
                let Some(f) = active() else { continue };
                if vu.as_ref() == Some(&f) {
                    continue;
                }
                // L Katta sait maintenant quelle fenêtre est devant, donc la règle de discrétion
                // s'applique : `pont_actif` devient vrai, c'est exactement ce qu'il dit.
                super::PONT.store(true, std::sync::atomic::Ordering::Relaxed);
                vu = Some(f.clone());
                tx(Msg::Fenetre(f));
            }
        });
    }

    /// La fenêtre de premier plan, en pixels physiques et dans le repère de l'écran — les mêmes
    /// coordonnées globales que le script KWin envoie sous Linux. La conversion vers le monde du
    /// maki est faite par celui qui connaît sa zone de travail, pas ici.
    fn active() -> Option<Fenetre> {
        // SAFETY : le handle appartient au système, il n'y a rien à libérer.
        let f = unsafe { GetForegroundWindow() };
        // Réduite en icône : ce n'est pas une fenêtre devant l'utilisateur, et son rectangle ne
        // veut plus rien dire (Windows la gare hors écran, à -32000).
        if f == 0 || unsafe { IsIconic(f) } != 0 {
            return None;
        }
        let classe = texte(f, true);
        // Le maki, la barre des tâches et le tiroir d'icônes : on garde la fenêtre précédente,
        // comme le script KWin ignore `dock` et `notification`.
        if matches!(classe.as_str(), NOTRE_CLASSE | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "NotifyIconOverflowWindow") {
            return None;
        }
        // Le fond d'écran (`Progman`, `WorkerW`) couvre tout le moniteur : sans ce filtre, cliquer
        // sur le bureau ferait croire à un film en plein écran et ferait disparaître le maki.
        if matches!(classe.as_str(), "Progman" | "WorkerW") {
            return Some(Fenetre::default());
        }
        let titre = texte(f, false);
        let mut r = Rect::default();
        // SAFETY : écrit un RECT dans notre variable, qui vit jusqu'à la fin de l'appel.
        if unsafe { GetWindowRect(f, &mut r) } == 0 {
            return None;
        }
        let ecran = crate::os::win::moniteur_du_point(r.left + (r.right - r.left) / 2, r.top + (r.bottom - r.top) / 2).unwrap_or_default();
        Some(Fenetre {
            plein_ecran: couvre_le_moniteur(f, &r),
            rect: (r.left as f32, r.top as f32, (r.right - r.left) as f32, (r.bottom - r.top) as f32),
            classe,
            titre,
            ecran,
            scene: (0.0, 0.0),
        })
    }

    /// Plein écran : la fenêtre couvre tout le moniteur, pas seulement sa zone de travail.
    ///
    /// Une fenêtre maximisée s'arrête à la barre des tâches, donc elle couvre `rc_work` et pas
    /// `rc_monitor`. Toute la distinction tient là, et elle compte : comparer au mauvais rectangle
    /// ferait croire à L Katta que l'utilisateur est en plein écran dès qu'il agrandit une fenêtre,
    /// et le réduirait au silence pour un oui pour un non.
    fn couvre_le_moniteur(f: isize, r: &Rect) -> bool {
        let mut info = InfoMoniteur {
            cb_size: std::mem::size_of::<InfoMoniteur>() as u32,
            rc_monitor: Rect::default(),
            rc_work: Rect::default(),
            dw_flags: 0,
        };
        // SAFETY : `f` vient de `GetForegroundWindow` et n'est pas nul ; `MONITOR_DEFAULTTONEAREST`
        // garantit un handle valable même si la fenêtre est hors écran.
        let m = unsafe { MonitorFromWindow(f, MONITOR_DEFAULTTONEAREST) };
        // SAFETY : `info` est à nous et vit jusqu'à la fin de l'appel.
        if m == 0 || unsafe { GetMonitorInfoW(m, &mut info as *mut InfoMoniteur as *mut std::ffi::c_void) } == 0 {
            // Sans renseignement sur le moniteur, mieux vaut dire « pas plein écran » : se taire à
            // tort est plus gênant que parler à tort.
            return false;
        }
        super::couvre_tout(
            (r.left, r.top, r.right, r.bottom),
            (info.rc_monitor.left, info.rc_monitor.top, info.rc_monitor.right, info.rc_monitor.bottom),
        )
    }

    /// Le titre (`classe` faux) ou la classe (`classe` vrai) de la fenêtre.
    fn texte(f: isize, classe: bool) -> String {
        let mut tampon = vec![0u16; TAILLE_TEXTE];
        // SAFETY : la fonction écrit au plus `TAILLE_TEXTE` caractères dans notre tampon et rend
        // le nombre écrit, sans compter le NUL final.
        let n = if classe {
            unsafe { GetClassNameW(f, tampon.as_mut_ptr(), TAILLE_TEXTE as i32) }
        } else {
            unsafe { GetWindowTextW(f, tampon.as_mut_ptr(), TAILLE_TEXTE as i32) }
        };
        if n <= 0 {
            // Fenêtre sans titre (beaucoup de fenêtres d'outil) : vide, et non une erreur.
            return String::new();
        }
        // `.min` : ne jamais faire confiance à la longueur rendue au point de lire au-delà.
        let n = (n as usize).min(TAILLE_TEXTE);
        String::from_utf16_lossy(&tampon[..n])
    }
}

// --- raccourci global (KGlobalAccel) ----------------------------------------------------

#[cfg_attr(windows, allow(dead_code))]
const ACTION: &str = "['lkatta', 'discuter', 'L Katta', 'Ouvrir la bulle de discussion']";

/// « Meta+K », « Ctrl+Alt+Espace », « Meta+F2 »… en code de touche Qt (0 = invalide).
#[cfg_attr(windows, allow(dead_code))]
pub fn qt_key(seq: &str) -> Option<i32> {
    let mut code = 0i32;
    let mut key = None;
    for part in seq.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_lowercase().as_str() {
            "meta" | "super" | "win" => code |= 0x1000_0000,
            "ctrl" | "control" => code |= 0x0400_0000,
            "alt" => code |= 0x0800_0000,
            "shift" | "maj" => code |= 0x0200_0000,
            "space" | "espace" => key = Some(0x20),
            p if p.len() == 1 && p.chars().all(|c| c.is_ascii_alphanumeric()) => key = Some(p.to_ascii_uppercase().as_bytes()[0] as i32),
            p if p.starts_with('f') && p[1..].parse::<i32>().is_ok_and(|n| (1..=24).contains(&n)) => key = Some(0x0100_0030 + p[1..].parse::<i32>().unwrap() - 1),
            _ => return None,
        }
    }
    key.map(|k| code | k)
}

#[cfg_attr(windows, allow(dead_code))]
fn kga(method: &str, args: &[&str]) -> Option<String> {
    let o = crate::hote::commande("gdbus")
        .args(["call", "--session", "--dest", "org.kde.kglobalaccel", "--object-path", "/kglobalaccel", "--method", &format!("org.kde.KGlobalAccel.{method}")])
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Enregistre le raccourci dans Plasma et attend qu'on le presse (fil dédié).
pub fn raccourci(seq: &str, tx: impl Fn(Msg) + Send + 'static) {
    // Sous Windows, le raccourci est enregistré directement sur la fenêtre du maki (`RegisterHotKey`),
    // et mis à jour à chaud dès que `config.toml` change.
    #[cfg(windows)]
    {
        let _ = (seq, tx);
        return;
    }
    // GNOME : raccourci personnalisé qui lance `lkatta raccourci` (pas de KGlobalAccel).
    #[cfg(not(windows))]
    {
    if crate::gnome::bureau_gnome() {
        let seq = seq.to_string();
        std::thread::spawn(move || crate::gnome::raccourci(&seq));
        return;
    }
    if !which("gdbus") {
        return;
    }
    let seq = seq.trim().to_string();
    std::thread::spawn(move || {
        if seq.is_empty() || seq == "aucun" {
            let _ = kga("unRegister", &[ACTION]);
            return;
        }
        let Some(k) = qt_key(&seq) else {
            eprintln!("lkatta : raccourci « {seq} » incompris (ex. Meta+K)");
            return;
        };
        let keys = format!("[{k}]");
        let _ = kga("doRegister", &[ACTION]);
        // Par défaut, puis actif (2 = présent, 4 = sans relire l'ancien : la config fait foi).
        let _ = kga("setShortcut", &[ACTION, &keys, "1"]);
        let _ = kga("setShortcut", &[ACTION, &keys, "6"]);
        let Ok(mut child) = paths::mourir_avec_parent(&mut crate::hote::commande("gdbus"))
            .args(["monitor", "--session", "--dest", "org.kde.kglobalaccel", "--object-path", "/component/lkatta"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        let out = child.stdout.take().expect("stdout");
        use std::io::{BufRead, BufReader};
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if line.contains("globalShortcutPressed") && line.contains("'discuter'") {
                tx(Msg::Raccourci);
            }
        }
        let _ = child.wait();
    });
    }
}

/// Donne le clavier à la bulle (ouverte par le raccourci) : un petit script KWin active la
/// surface du maki. Elle reste « OnDemand » : un clic ailleurs rend le clavier.
#[cfg_attr(windows, allow(dead_code))]
pub fn focaliser_bulle() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(120));
        const JS: &str = r#"var l = workspace.windowList(), best = null, a = 0;
for (var i = 0; i < l.length; i++) {
    var w = l[i], g = w.frameGeometry;
    if (String(w.resourceClass) === "lkatta" && g.width * g.height > a) { best = w; a = g.width * g.height; }
}
if (best) workspace.activeWindow = best;
"#;
        let path = paths::data_dir().join("kwin-lkatta-focus.js");
        if paths::write_private(&path, JS.as_bytes()).is_err() {
            return;
        }
        let p = path.to_string_lossy().into_owned();
        let _ = qdbus(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.unloadScript", "lkatta-focus"]);
        let _ = qdbus(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.loadScript", &p, "lkatta-focus"]);
        let _ = qdbus(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.start"]);
        std::thread::sleep(Duration::from_millis(400));
        let _ = qdbus(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.unloadScript", "lkatta-focus"]);
    });
}

/// « Ne pas déranger » activé (Plasma, GNOME, ou mode présentation / concentration de Windows) ?
pub fn ne_pas_deranger() -> bool {
    #[cfg(windows)]
    return crate::os::win::ne_pas_deranger();
    #[cfg(not(windows))]
    {
        if crate::gnome::bureau_gnome() {
            return crate::gnome::ne_pas_deranger();
        }
        crate::hote::commande("gdbus")
            .args(["call", "--session", "--dest", "org.freedesktop.Notifications", "--object-path", "/org/freedesktop/Notifications", "--method", "org.freedesktop.DBus.Properties.Get", "org.freedesktop.Notifications", "Inhibited"])
            .stderr(Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("true"))
            .unwrap_or(false)
    }
}

/// Le micro est-il utilisé (enregistrement en cours : visio, appel) ?
pub fn micro_utilise() -> bool {
    #[cfg(windows)]
    return crate::os::win::micro_utilise();
    #[cfg(not(windows))]
    {
        if !which("pactl") {
            return false;
        }
        crate::hote::commande("pactl")
            .args(["list", "source-outputs", "short"])
            .stderr(Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| !l.trim().is_empty()))
            .unwrap_or(false)
    }
}

/// Veille légère : « Ne pas déranger » et micro, toutes les 3 s, signalés quand ils changent.
pub fn veiller(tx: impl Fn(Msg) + Send + 'static) {
    std::thread::spawn(move || {
        let mut last = (false, false);
        loop {
            let now = (ne_pas_deranger(), micro_utilise());
            if now != last {
                last = now;
                tx(Msg::Occupe { ne_pas_deranger: now.0, micro: now.1 });
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_calls_and_presentations() {
        let f = |c: &str, t: &str| Fenetre { classe: c.into(), titre: t.into(), ..Default::default() };
        assert!(f("zoom", "Réunion Zoom").visio());
        assert!(f("firefox", "Meet - abc-defg-hij").visio());
        assert!(f("libreoffice-impress", "cours.odp").visio());
        assert!(!f("org.kde.dolphin", "Documents").visio());
        assert_eq!(qt_key("Meta+K"), Some(0x1000_004B));
        assert_eq!(qt_key("ctrl+alt+espace"), Some(0x0C00_0020));
        assert_eq!(qt_key("Meta+F2"), Some(0x1000_0000 | 0x0100_0031));
        assert_eq!(qt_key("Meta+Truc"), None);
        let j = Fenetre::from_json(r#"{"plein":true,"x":0,"y":0,"w":1920,"h":1080,"classe":"mpv","titre":"film","ecran":"eDP-1"}"#).unwrap();
        assert!(j.plein_ecran && j.rect.2 == 1920.0 && j.ecran == "eDP-1");
        // Plein écran : la fenêtre couvre le moniteur entier.
        assert!(couvre_tout((0, 0, 2560, 1440), (0, 0, 2560, 1440)));
        // Maximisée : la barre des tâches en bas reste libre, donc ce n'est pas du plein écran.
        assert!(!couvre_tout((0, 0, 2560, 1400), (0, 0, 2560, 1440)));
        // Fenêtre ordinaire, ni plus large ni plus haute que le moniteur.
        assert!(!couvre_tout((200, 100, 1200, 900), (0, 0, 2560, 1440)));
        // Bord à bord avec une barre des tâches sur le côté : toujours pas plein écran.
        assert!(!couvre_tout((0, 0, 2520, 1440), (0, 0, 2560, 1440)));
        // Qui déborde de partout (fenêtre plus grande que l'écran) : plein écran.
        assert!(couvre_tout((-40, -20, 2600, 1500), (0, 0, 2560, 1440)));
        // Un moniteur qui n'est pas à l'origine : le repère importe, pas les valeurs absolues.
        assert!(couvre_tout((2560, 0, 5120, 1440), (2560, 0, 5120, 1440)));
        assert!(!couvre_tout((2560, 0, 5120, 1400), (2560, 0, 5120, 1440)));
    }
}
