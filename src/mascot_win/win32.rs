//! La fenêtre du maki sous Windows : une fenêtre en calques, et sa boucle de messages.
//!
//! ## Le choix de la fenêtre en calques
//!
//! `WS_EX_LAYERED` avec `UpdateLayeredWindow` et une alpha par pixel. C'est ce qu'il faut pour un
//! compagnon de bureau : la fenêtre est traversée par la souris partout où l'alpha est nul, donc
//! le maki n'a pas de rectangle invisible qui volerait les clics des applications derrière lui.
//! Aucun `WM_NCHITTEST` à écrire, aucune région à recalculer à chaque image — Windows le fait à
//! partir des pixels eux-mêmes.
//!
//! Elle est aussi `WS_EX_NOACTIVATE` (prendre le clavier de l'utilisateur pour un lémurien serait
//! une faute), `WS_EX_TOOLWINDOW` (absente de la barre des tâches) et `WS_EX_TOPMOST`.
//!
//! ## tiny-skia produit déjà ce que Windows attend
//!
//! `UpdateLayeredWindow` avec `AC_SRC_ALPHA` veut du BGRA **prémultiplié**. tiny-skia produit du
//! RGBA prémultiplié : il n'y a donc que deux octets à échanger par pixel, et aucune conversion
//! d'alpha à refaire. C'est exactement ce que fait déjà le backend X11 (`mascot/x11.rs`), pour la
//! même raison.
//!
//! ## Coût
//!
//! La surface part de 470×560 pixels logiques, comme sous Wayland, multipliés par le rapport
//! d'échelle de l'écran, puis grandit pour contenir la liane, l'arbre ou la bulle de discussion.
//! En 100 % cela donne un peu plus d'un méga-octet pour l'image et autant pour le bitmap système
//! — très en dessous du budget de 15 Mo. La fenêtre est déplacée pour suivre le maki plutôt que
//! d'occuper tout l'écran, ce qui est aussi ce que font les backends Wayland et X11.
//!
//! ## Ce qui n'est pas câblé
//!
//! Les boutons d'action sous une bulle spontanée : `actions_for` vit dans `mascot/mod.rs` et
//! dépend de ses outils (historique, fils, courriel). Tout le reste — la discussion, ses réglages,
//! le clavier, le presse-papiers, les fichiers joints, l'historique des fils — passe par `chat` et
//! `draw`, qui sont portables, et est branché ici.

use std::cell::RefCell;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use tiny_skia::{Pixmap, Transform};

use super::behavior::{Ctx, Exits, Maki, PoseKind, Tree};
use super::draw::{self, BubbleLayout, ChatLayout, R, Text, hit};
use super::especes::Espece;
use crate::chat::{Chat, Signal};
use crate::config::Config;
use crate::conseils::{Kind, Message};
use crate::ipc::Msg;

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(classe: *const WNDCLASSEXW) -> u16;
    fn CreateWindowExW(
        style_etendu: u32,
        classe: *const u16,
        titre: *const u16,
        style: u32,
        x: i32,
        y: i32,
        largeur: i32,
        hauteur: i32,
        parent: isize,
        menu: isize,
        instance: isize,
        param: *mut c_void,
    ) -> isize;
    fn DefWindowProcW(fenetre: isize, message: u32, w: usize, l: isize) -> isize;
    fn GetMessageW(m: *mut MSG, fenetre: isize, filtre_min: u32, filtre_max: u32) -> i32;
    fn TranslateMessage(m: *const MSG) -> i32;
    fn DispatchMessageW(m: *const MSG) -> isize;
    fn PostQuitMessage(code: i32);
    fn PostMessageW(fenetre: isize, message: u32, w: usize, l: isize) -> i32;
    fn DestroyWindow(fenetre: isize) -> i32;
    fn SetTimer(fenetre: isize, id: usize, millisecondes: u32, rappel: usize) -> usize;
    fn KillTimer(fenetre: isize, id: usize) -> i32;
    fn GetDC(fenetre: isize) -> isize;
    fn ReleaseDC(fenetre: isize, dc: isize) -> i32;
    fn GetDeviceCaps(dc: isize, index: i32) -> i32;
    fn UpdateLayeredWindow(
        fenetre: isize,
        dc_cible: isize,
        destination: *const POINT,
        taille: *const SIZE,
        dc_source: isize,
        source: *const POINT,
        cle: u32,
        fusion: *const BLENDFUNCTION,
        drapeaux: u32,
    ) -> i32;
    fn SystemParametersInfoW(action: u32, param: u32, out: *mut c_void, drapeaux: u32) -> i32;
    fn SetWindowPos(fenetre: isize, apres: isize, x: i32, y: i32, cx: i32, cy: i32, drapeaux: u32) -> i32;
    fn GetWindowLongPtrW(fenetre: isize, index: i32) -> isize;
    fn SetWindowLongPtrW(fenetre: isize, index: i32, valeur: isize) -> isize;
    fn SetForegroundWindow(fenetre: isize) -> i32;
    fn SetFocus(fenetre: isize) -> isize;
    fn GetKeyState(touche: i32) -> i16;
    fn GetCursorPos(p: *mut POINT) -> i32;
    fn SetCapture(fenetre: isize) -> isize;
    fn ReleaseCapture() -> i32;
    fn TrackMouseEvent(e: *mut TRACKMOUSEEVENT) -> i32;
    fn LoadCursorW(instance: isize, nom: *const u16) -> isize;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(dc: isize) -> isize;
    fn CreateDIBSection(dc: isize, info: *const BITMAPINFOHEADER, usage: u32, bits: *mut *mut c_void, section: isize, decalage: u32) -> isize;
    fn SelectObject(dc: isize, objet: isize) -> isize;
    fn DeleteObject(objet: isize) -> i32;
    fn DeleteDC(dc: isize) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(nom: *const u16) -> isize;
}

#[repr(C)]
#[derive(Clone, Copy)]
struct POINT {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SIZE {
    cx: i32,
    cy: i32,
}

/// Rectangle en coordonnées d'écran. `Debug` parce que les essais l'affichent en cas d'échec.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

/// `MSG` : la disposition x86_64 fait 48 octets, `lPrivate` compris. Il est inutilisé mais fait
/// partie de la structure — l'omettre décalerait rien, ici, mais le garder coûte rien non plus et
/// évite une surprise si Windows écrit dedans.
#[repr(C)]
struct MSG {
    hwnd: isize,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    pt: POINT,
    lprivate: u32,
}

/// `WNDPROC` : la signature exacte que Windows attend du rappel de fenêtre.
///
/// La déclarer comme un type de fonction plutôt que comme un entier a deux effets. Le compilateur
/// vérifie que `procedure` a bien les quatre paramètres et le bon retour — avec un `usize` une
/// signature fausse ne se verrait qu'à l'appel, sous forme de pile corrompue. Et cela évite le
/// cast direct d'une fonction en entier, que rustc signale à juste titre.
type ProcedureFenetre = unsafe extern "system" fn(fenetre: isize, message: u32, w: usize, l: isize) -> isize;

#[repr(C)]
struct WNDCLASSEXW {
    cbsize: u32,
    style: u32,
    wndproc: ProcedureFenetre,
    cbclsextra: i32,
    cbwndextra: i32,
    hinstance: isize,
    hicon: isize,
    hcursor: isize,
    hbrbackground: isize,
    lpszmenuname: *const u16,
    lpszclassname: *const u16,
    hiconsm: isize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BLENDFUNCTION {
    operation: u8,
    drapeaux: u8,
    alpha: u8,
    format: u8,
}

#[repr(C)]
struct BITMAPINFOHEADER {
    taille: u32,
    largeur: i32,
    hauteur: i32,
    plans: u16,
    bits: u16,
    compression: u32,
    taille_image: u32,
    x_par_metre: i32,
    y_par_metre: i32,
    couleurs: u32,
    importantes: u32,
}

#[repr(C)]
struct TRACKMOUSEEVENT {
    cbsize: u32,
    drapeaux: u32,
    fenetre: isize,
    survol: u32,
}

const WS_POPUP: u32 = 0x8000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_EX_LAYERED: u32 = 0x0008_0000;
const WS_EX_TOPMOST: u32 = 0x0000_0008;
const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
const WS_EX_NOACTIVATE: u32 = 0x0800_0000;

const WM_DESTROY: u32 = 0x0002;
const WM_TIMER: u32 = 0x0113;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_MBUTTONUP: u32 = 0x0208;
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_SETFOCUS: u32 = 0x0007;
const WM_KILLFOCUS: u32 = 0x0008;
const WM_APPLIQUER_CLAVIER: u32 = 0x8001;
const WM_KEYDOWN: u32 = 0x0100;
const WM_CHAR: u32 = 0x0102;

/// Attribut de style étendu : c'est lui qui dit si la fenêtre peut être activée, donc si elle
/// reçoit des touches.
const GWL_EXSTYLE: i32 = -20;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;

/// Touches virtuelles. Les lettres ont le code de leur majuscule ASCII.
const VK_SHIFT: i32 = 0x10;
const VK_CONTROL: i32 = 0x11;
const VK_MENU: i32 = 0x12;
const VK_BACK: usize = 0x08;
const VK_RETURN: usize = 0x0D;
const VK_ESCAPE: usize = 0x1B;
const VK_PRIOR: usize = 0x21;
const VK_NEXT: usize = 0x22;
const VK_END: usize = 0x23;
const VK_HOME: usize = 0x24;
const VK_LEFT: usize = 0x25;
const VK_UP: usize = 0x26;
const VK_RIGHT: usize = 0x27;
const VK_DOWN: usize = 0x28;
const VK_DELETE: usize = 0x2E;
const VK_A: usize = 0x41;
const VK_C: usize = 0x43;
const VK_O: usize = 0x4F;
const VK_U: usize = 0x55;
const VK_V: usize = 0x56;
const VK_X: usize = 0x58;

/// Au-delà de ce déplacement, un appui devient un glisser. Sans cette marge, le moindre frémis
/// de la main attraperait le maki au lieu d'ouvrir la discussion.
const MARGE_GLISSER: f32 = 4.0;

const LOGPIXELSX: i32 = 88;
const SPI_GETWORKAREA: u32 = 0x0030;
const DIB_RGB_COLORS: u32 = 0;
const BI_RGB: u32 = 0;
const ULW_ALPHA: u32 = 0x0000_0002;
const AC_SRC_OVER: u8 = 0x00;
const AC_SRC_ALPHA: u8 = 0x01;
const TME_LEAVE: u32 = 0x0000_0002;
const IDC_ARROW: usize = 32512;

/// Images par seconde. Assez pour que la démarche soit fluide, assez peu pour ne pas occuper un
/// processeur qui ne fait rien : le maki dort souvent, et chaque image coûte un plein écran de
/// calculs de pixels.
const MILLIS_PAR_IMAGE: u32 = 33;
const ID_MINUTEUR: usize = 1;

thread_local! {
    /// L'état du maki, accessible au rappel de fenêtre.
    ///
    /// `thread_local` et non une variable globale : la boucle de messages et le rappel sont sur le
    /// même fil, donc il n'y a jamais deux accès simultanés, et aucun verrou à prendre.
    static ETAT: RefCell<Option<Etat>> = const { RefCell::new(None) };
}

/// Lance le maki et ne revient qu'à sa fermeture.
pub fn run(cfg: Config, rx: crate::os::canal::Recepteur<Msg>) -> Result<(), String> {
    let espece = Espece::parse(&cfg.maki.espece).unwrap_or(Espece::Catta);
    draw::set_theme(&cfg.maki.theme);
    draw::set_species(espece);

    let texte = Text::load().ok_or("la police intégrée n'a pas pu être chargée")?;
    // `load_bold` est une fonction libre de `draw`, pas une méthode de `Text`. Si la police grasse
    // intégrée ne se charge pas, on retombe sur la régulière plutôt que de refuser de démarrer :
    // un maki sans titres gras vaut mieux qu'un compagnon qui ne se lance pas.
    let gras = draw::load_bold().or_else(Text::load).ok_or("aucune police intégrée n'a pu être chargée")?;

    let travail = zone_de_travail();
    let monde = ((travail.right - travail.left) as f32, (travail.bottom - travail.top) as f32);
    let echelle = rapport_echelle();
    // Même calcul que sous Wayland : la taille demandée, pondérée par l'espèce, bornée.
    let s = (cfg.maki.taille * espece.pal().size).clamp(0.4, 3.5) * echelle;

    let (largeur, hauteur) = (((470.0 * s) as u32).max(64), ((560.0 * s) as u32).max(64));
    let pixmap = Pixmap::new(largeur, hauteur).ok_or("taille de surface invalide")?;
    let sprite = Pixmap::new(largeur, hauteur).ok_or("taille de calque invalide")?;

    let mut maki = Maki::new(monde.0 * 0.66, s, fastrand::u64(..));
    maki.sp = espece;

    let (dc, bit, bits) = dib(largeur, hauteur)?;
    let chat = Chat::new(cfg.clone());

    let etat = Etat {
        cfg,
        chat,
        chat_layout: None,
        bulle_layout: None,
        a_le_clavier: false,
        appui: None,
        tampon_utf16: Vec::new(),
        echelle,
        rx,
        maki,
        pixmap,
        sprite,
        texte,
        gras,
        dc,
        bit,
        bits,
        largeur,
        hauteur,
        s,
        monde,
        travail,
        origin: (0.0, 0.0),
        fenetre: 0,
        devant: None,
        ne_pas_deranger: false,
        micro: false,
        pointeur: None,
        survole: false,
        entraine: false,
        traine: false,
        quiet: false,
        cache: false,
        en_attente: Vec::new(),
        etait_occupe: false,
        derniere_parole: None,
        place: false,
        derniere: Instant::now(),
    };

    // L'état est posé AVANT la création de la fenêtre : Windows envoie WM_NCCREATE et WM_CREATE
    // pendant `CreateWindowExW`, et le rappel les lit. Inverser ferait paniquer l'emprunt.
    ETAT.with(|e| *e.borrow_mut() = Some(etat));

    let nom_classe = large("LkattaMaki");
    // SAFETY : `GetModuleHandleW(null)` renvoie l'instance du processus en cours, toujours
    // valide ; la structure de classe pointe vers des textes qui vivent jusqu'à l'appel.
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let classe = WNDCLASSEXW {
        cbsize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        wndproc: procedure,
        cbclsextra: 0,
        cbwndextra: 0,
        hinstance: instance,
        hicon: 0,
        // SAFETY : MAKEINTRESOURCE(32512) est un curseur système, pas un pointeur à déréférencer.
        hcursor: unsafe { LoadCursorW(0, IDC_ARROW as *const u16) },
        // Aucun fond : la fenêtre est un calque, son contenu vient d'`UpdateLayeredWindow`.
        hbrbackground: 0,
        lpszmenuname: std::ptr::null(),
        lpszclassname: nom_classe.as_ptr(),
        hiconsm: 0,
    };
    // SAFETY : enregistre la classe ; 0 en cas d'échec.
    if unsafe { RegisterClassExW(&classe) } == 0 {
        return Err("la classe de fenêtre n'a pas pu être enregistrée".into());
    }

    // SAFETY : création d'une fenêtre surgissante sans parent ; 0 en cas d'échec.
    let fenetre = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            nom_classe.as_ptr(),
            std::ptr::null(),
            WS_POPUP | WS_VISIBLE,
            travail.left,
            travail.top,
            largeur as i32,
            hauteur as i32,
            0,
            0,
            instance,
            std::ptr::null_mut(),
        )
    };
    if fenetre == 0 {
        return Err("la fenêtre du maki n'a pas pu être créée".into());
    }

    ETAT.with(|e| {
        if let Some(a) = &mut *e.borrow_mut() {
            a.fenetre = fenetre;
        }
    });

    // Fait apparaître le maki : `Maki::new` le crée caché, c'est `enter` qui le pose au sol.
    ETAT.with(|e| {
        if let Some(a) = &mut *e.borrow_mut() {
            let ctx = a.ctx();
            a.maki.enter(&ctx);
        }
    });

    // SAFETY : minuteur de fenêtre, sans rappel : c'est WM_TIMER qui passera par `procedure`.
    unsafe { SetTimer(fenetre, ID_MINUTEUR, MILLIS_PAR_IMAGE, 0) };

    let mut m = MSG {
        hwnd: 0,
        message: 0,
        wparam: 0,
        lparam: 0,
        time: 0,
        pt: POINT { x: 0, y: 0 },
        lprivate: 0,
    };
    loop {
        // SAFETY : `GetMessageW` remplit notre structure ; -1 signale une erreur, 0 la fin.
        let r = unsafe { GetMessageW(&mut m, 0, 0, 0) };
        if r <= 0 {
            break;
        }
        // SAFETY : répartition standard du message à sa fenêtre.
        unsafe {
            TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }

    // SAFETY : le minuteur, le bitmap et le DC ont été créés ici, et la fenêtre va disparaître.
    unsafe {
        KillTimer(fenetre, ID_MINUTEUR);
    }
    ETAT.with(|e| {
        if let Some(a) = e.borrow_mut().take() {
            // SAFETY : `a.dc`, `a.bit` viennent de `CreateCompatibleDC` et `CreateDIBSection`,
            // supprimés une seule fois, ici, après la fin de la boucle de messages.
            unsafe {
                DeleteDC(a.dc);
                DeleteObject(a.bit);
            }
        }
    });
    Ok(())
}

/// Le rappel de fenêtre. Tout y est court : le travail est dans `Etat`, emprunté le temps d'un
/// appel, jamais gardé.
unsafe extern "system" fn procedure(fenetre: isize, message: u32, w: usize, l: isize) -> isize {
    match message {
        WM_TIMER => {
            avec_etat(|a| a.tic());
            0
        }
        WM_MOUSEMOVE => {
            avec_etat(|a| {
                a.souris();
                // Un appui qui bouge devient un glisser : au-delà de la marge, on attrape le maki.
                if let Some((depart, false)) = a.appui {
                    let ici = local(l);
                    if (ici.0 - depart.0).abs() > MARGE_GLISSER || (ici.1 - depart.1).abs() > MARGE_GLISSER {
                        a.appui = Some((depart, true));
                        a.prendre();
                    }
                }
            });
            0
        }
        WM_MOUSELEAVE => {
            avec_etat(|a| {
                a.survole = false;
                a.pointeur = None;
                a.entraine = false;
            });
            0
        }
        WM_LBUTTONDOWN => {
            // SAFETY : capture sur notre fenêtre, pour que le glisser continue hors du maki.
            unsafe { SetCapture(fenetre) };
            // L'appui est seulement noté : s'il ne bouge pas c'est un clic, qui ouvre la
            // discussion ; s'il bouge c'est un glisser, qui attrape le maki. Attraper dès l'appui
            // rendrait tout clic impossible — c'est ce que faisait cette fenêtre avant.
            avec_etat(|a| a.appui = Some((local(l), false)));
            0
        }
        WM_LBUTTONUP => {
            // SAFETY : fin de la capture prise à WM_LBUTTONDOWN.
            unsafe { ReleaseCapture() };
            avec_etat(|a| {
                let glisse = a.appui.map(|(_, g)| g).unwrap_or(false);
                a.appui = None;
                if glisse {
                    a.poser();
                } else {
                    a.clic(local(l));
                }
            });
            0
        }
        WM_RBUTTONUP => {
            // Clic droit : le mode discret, comme sous Linux. Ce n'est pas un salut — un bouton qui
            // ne fait rien d'utile apprend seulement à l'utilisateur à ne pas s'en servir.
            avec_etat(|a| {
                a.quiet = !a.quiet;
                let t = if a.quiet { t!("Je me fais tout discret.") } else { t!("Me revoilà !") };
                a.maki.say(t, 4.0);
            });
            0
        }
        WM_MBUTTONUP => {
            // Clic du milieu : un numéro au hasard, comme sous Linux.
            avec_etat(|a| {
                let ctx = a.ctx();
                match fastrand::u8(0..4) {
                    0 => a.maki.tree_show(&ctx),
                    1 => a.maki.teleport(&ctx),
                    2 => a.maki.climb_edge(&ctx),
                    _ => a.maki.flip(),
                }
            });
            0
        }
        WM_APPLIQUER_CLAVIER => {
            // Ne pas tenir un emprunt de ETAT : ces appels peuvent envoyer WM_SETFOCUS.
            unsafe {
                let ex = GetWindowLongPtrW(fenetre, GWL_EXSTYLE);
                let nouveau = if w != 0 { ex & !(WS_EX_NOACTIVATE as isize) } else { ex | WS_EX_NOACTIVATE as isize };
                SetWindowLongPtrW(fenetre, GWL_EXSTYLE, nouveau);
                if w != 0 {
                    SetForegroundWindow(fenetre);
                    SetFocus(fenetre);
                }
            }
            0
        }
        WM_SETFOCUS => {
            avec_etat(|a| { a.a_le_clavier = true; a.chat.retour_attendu = false; });
            0
        }
        WM_KILLFOCUS => {
            // Le clavier part ailleurs : la discussion se referme. C'est le comportement de la
            // version Linux, où la bulle ouverte par un clic « se referme quand on clique
            // ailleurs » — ici l'ailleurs se voit directement, par la perte du focus.
            avec_etat(|a| {
                a.a_le_clavier = false;
                if a.chat.fermable() {
                    a.basculer_chat();
                }
            });
            0
        }
        WM_KEYDOWN => {
            avec_etat(|a| a.touche(w));
            0
        }
        WM_CHAR => {
            avec_etat(|a| a.caractere(w as u16));
            0
        }
        WM_DESTROY => {
            // SAFETY : demande la fin de la boucle de messages.
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(fenetre, message, w, l) },
    }
}

/// Emprunte l'état le temps d'un appel. Un emprunt jamais gardé : le rappel peut être rappelé
/// par Windows pendant un traitement, et une seconde `borrow_mut` paniquerait.
fn avec_etat(f: impl FnOnce(&mut Etat)) {
    ETAT.with(|e| {
        if let Ok(mut etat) = e.try_borrow_mut() {
            if let Some(a) = &mut *etat {
                f(a);
            }
        }
    });
}

struct Etat {
    cfg: Config,
    rx: crate::os::canal::Recepteur<Msg>,
    maki: Maki,
    pixmap: Pixmap,
    sprite: Pixmap,
    texte: Text,
    gras: Text,
    /// DC mémoire et bitmap qui portent l'image, créés une fois pour toutes.
    dc: isize,
    bit: isize,
    bits: *mut u8,
    largeur: u32,
    hauteur: u32,
    /// Taille du maki, rapport d'échelle de l'écran déjà appliqué.
    s: f32,
    /// Taille du monde : la zone de travail du moniteur, en pixels physiques.
    monde: (f32, f32),
    travail: RECT,
    /// Coin haut-gauche de la fenêtre dans le monde.
    origin: (f32, f32),
    fenetre: isize,
    /// La fenêtre qui est devant, telle que `bureau::demarrer` la sonde. `None` jusqu'à la
    /// première réponse du système : jusque-là le maki n'évite rien et ne monte sur rien, plutôt
    /// que d'éviter une fenêtre inventée.
    devant: Option<crate::bureau::Fenetre>,
    /// La discussion, son tracé de la dernière image (pour savoir où sont ses boutons), et si la
    /// fenêtre tient le clavier en ce moment.
    chat: Chat,
    chat_layout: Option<ChatLayout>,
    bulle_layout: Option<BubbleLayout>,
    a_le_clavier: bool,
    /// Appui en cours : `(point dans la surface, déjà bougé)`. Un appui qui ne bouge pas est un
    /// clic et ouvre la discussion ; dès qu'il bouge, il attrape le maki.
    appui: Option<((f32, f32), bool)>,
    /// Moitiés d'un caractère hors BMP, le temps que la seconde arrive.
    tampon_utf16: Vec<u16>,
    /// Rapport d'échelle de l'écran, gardé à part de `s` parce que le texte des bulles s'applique
    /// sur la taille demandée, pas sur celle du personnage.
    echelle: f32,
    /// « Ne pas déranger » et micro occupé, poussés par `bureau::veiller`. Les deux sondes
    /// passent par `gdbus` et `pactl`, qui n'existent pas ici : elles rendent toujours faux sous
    /// Windows, donc ces deux booléens n'y changent jamais. Les traiter quand même coûte deux
    /// lignes et garde la même règle des deux côtés — le jour où une sonde native existe, rien
    /// d'autre ne bouge.
    ne_pas_deranger: bool,
    micro: bool,
    pointeur: Option<(f32, f32)>,
    survole: bool,
    entraine: bool,
    traine: bool,
    quiet: bool,
    cache: bool,
    /// La fenêtre a déjà été placée une fois : avant cela, tout contenu tient par définition.
    place: bool,
    /// Messages gardés pendant une période occupée : ils s'affichent à la fin, en une seule bulle,
    /// plutôt que de tomber au milieu d'une visioconférence.
    en_attente: Vec<Message>,
    /// Il était occupé à l'image précédente : la transition libère ce qui attendait.
    etait_occupe: bool,
    /// Dernière parole spontanée, pour respecter l'intervalle `parole_min`.
    derniere_parole: Option<Instant>,
    derniere: Instant,
}

impl Etat {
    /// Une image : lire les messages du démon, avancer le maki, dessiner, afficher.
    fn tic(&mut self) {
        let maintenant = Instant::now();
        let dt = maintenant.duration_since(self.derniere).as_secs_f32().clamp(0.0, 0.1);
        self.derniere = maintenant;

        self.lire_le_demon();
        self.sonder();
        let ctx = self.ctx();
        self.maki.step(dt, ctx);
        // La taille et la place dépendent de ce qu'il faut dessiner, donc de la pose : elles
        // viennent après le pas d'animation, comme dans la version Linux.
        self.ajuster();
        self.placer();
        self.peindre();
        self.presenter();
    }

    /// Vide le canal. Tout ce qui arrive d'un coup est traité : le démon peut envoyer plusieurs
    /// messages entre deux images, et en perdre un serait perdre une parole de l'utilisateur.
    fn lire_le_demon(&mut self) {
        loop {
            let Ok(m) = self.rx.try_recv() else { break };
            self.message(m);
        }
    }

    fn message(&mut self, m: Msg) {
        let ctx = self.ctx();
        match m {
            Msg::Say(t) => self.montrer(Message::new(Kind::Info, "", t)),
            Msg::Reminder(t) => self.montrer(Message::new(Kind::Reminder, t!("Rappel"), t)),
            // Une remarque spontanée est soumise à la discrétion et à l'intervalle entre paroles.
            Msg::Hint(t) => self.spontane(Message::new(Kind::Info, "", t)),
            // Occupé : astuces, nouvelles et mails attendent la fin ; les rappels passent.
            Msg::Bubble(m, _) if self.occupe() && matches!(m.kind, Kind::Mail | Kind::News | Kind::Tip | Kind::Alert) => {
                self.mettre_de_cote(m)
            }
            Msg::Bubble(m, true) => self.montrer(m),
            Msg::Bubble(m, false) => self.spontane(m),
            // Une discussion ouverte ou une réponse en cours : pas d'acrobatie par-dessus.
            Msg::Flip | Msg::Tree | Msg::Teleport | Msg::Climb if self.fige() => {}
            Msg::Flip => self.maki.flip(),
            Msg::Wave => self.maki.wave(),
            Msg::Tree => self.maki.tree_show(&ctx),
            Msg::Teleport => self.maki.teleport(&ctx),
            Msg::Climb => self.maki.climb_edge(&ctx),
            Msg::Quiet(v) => self.quiet = v.unwrap_or(!self.quiet),
            Msg::Hide(v) => {
                self.cache = v.unwrap_or(!self.cache);
                if !self.cache {
                    // Réapparaître se voit : le nuage, plutôt qu'un retour instantané.
                    self.maki.enter(&ctx);
                }
            }
            Msg::Reload => self.recharger(),
            Msg::Quit => {
                // SAFETY : détruit notre fenêtre, ce qui fait envoyer WM_DESTROY puis quitter la
                // boucle de messages.
                unsafe { DestroyWindow(self.fenetre) };
            }
            Msg::Fenetre(f) => self.sur_fenetre(f),
            Msg::Occupe { ne_pas_deranger, micro } => {
                self.ne_pas_deranger = ne_pas_deranger;
                self.micro = micro;
            }
            // Non câblés ici, et c'est écrit plutôt que laissé au hasard : `ActiveOutput`,
            // `Screen` et `Desktop` viennent du passage d'un écran ou d'un bureau à l'autre, et un
            // seul moniteur est pris en compte ; `Capture` sert aux essais X11.
            Msg::Chat | Msg::Raccourci => self.basculer_chat(),
            Msg::ActiveOutput(_) | Msg::Screen | Msg::Desktop(_) | Msg::Capture(_) => {}
        }
    }

    /// Le contexte du maki : ce qu'il sait du monde. Reprend les règles de la version Linux,
    /// moins ce que Windows ne lui donne pas encore.
    fn ctx(&self) -> Ctx {
        use chrono::Timelike;
        let heure = chrono::Local::now().hour();
        let [a, b] = self.cfg.maki.sommeil;
        let nuit = if a == b { false } else if a < b { heure >= a && heure < b } else { heure >= a || heure < b };
        Ctx {
            w: self.monde.0,
            h: self.monde.1,
            night: nuit,
            day: (6..18).contains(&heure),
            quiet: self.quiet || self.occupe(),
            acrobatics: self.cfg.maki.acrobaties,
            show_gap_min: self.cfg.maki.numeros_min.max(1) as f32,
            hovered: self.survole,
            pointer: self.pointeur,
            chatting: self.fige(),
            thinking: self.chat.busy,
            // Le pointeur est sur une bulle : elle ne s'efface pas tant qu'on la lit.
            bubble_hover: self.pointeur.zip(self.bulle_layout.as_ref()).is_some_and(|(p, l)| {
                hit(l.rect, (p.0 - self.origin.0, p.1 - self.origin.1))
            }),
            band: if self.cfg.maki.zone == "libre" { 0.0 } else { 0.17 },
            // Un seul moniteur pris en compte : aucune sortie voisine, donc il ne traverse pas.
            exits: Exits { left: false, right: false, up: false, down: false },
            avoid: self.a_eviter(),
            toit: if self.cfg.maki.acrobaties { self.toit() } else { None },
        }
    }

    /// Visioconférence, présentation ou plein écran : il se fait oublier.
    ///
    /// Sous Linux la règle ajoute « Ne pas déranger » et le micro occupé ; les deux booléens sont
    /// là mais leurs sondes (`gdbus`, `pactl`) n'existent pas sous Windows et rendent toujours
    /// faux. Ce qui fonctionne ici, et c'est l'essentiel de la règle, est la détection par le nom
    /// de la fenêtre : Zoom, Teams, Meet, Jitsi, Skype, Webex, Discord, OBS, Impress.
    fn occupe(&self) -> bool {
        self.ne_pas_deranger || self.micro || self.devant.as_ref().is_some_and(|f| f.plein_ecran || f.visio())
    }

    /// La discussion est ouverte, ou une réponse est en train d'arriver : il reste sur place.
    ///
    /// Même règle que la version Linux. Le glisser n'y entre pas volontairement : `chatting` arrête
    /// aussi la marche, et un maki qu'on déplace doit continuer à suivre la souris.
    fn fige(&self) -> bool {
        self.chat.open || self.chat.busy
    }

    /// Zone horizontale de la fenêtre de devant à éviter, sauf si elle prend presque toute la
    /// largeur : il n'y aurait alors nulle part où aller, et mieux vaut ignorer.
    ///
    /// Mêmes seuils que la version Linux, dans le monde du maki — la zone de travail, origine en
    /// haut à gauche, en pixels physiques.
    fn a_eviter(&self) -> Option<(f32, f32)> {
        let f = self.devant.as_ref()?;
        let w = self.monde.0;
        (f.rect.2 > 0.0 && f.rect.2 < w * 0.8).then_some((f.rect.0.max(0.0), (f.rect.0 + f.rect.2).min(w)))
    }

    /// Haut de la fenêtre de devant, s'il y a la place au-dessus pour lui : pas en plein écran,
    /// pas collée en haut, assez large. Mêmes seuils que la version Linux.
    fn toit(&self) -> Option<(f32, f32, f32)> {
        let f = self.devant.as_ref()?;
        if f.plein_ecran || f.rect.2 <= 0.0 {
            return None;
        }
        let (w, h) = self.monde;
        let s = self.s;
        let x0 = f.rect.0.max(0.0);
        let x1 = (f.rect.0 + f.rect.2).min(w);
        let y = f.rect.1;
        (x1 - x0 >= 260.0 * s && y >= 170.0 * s && y <= h - 120.0 * s).then_some((x0, x1, y))
    }

    /// Une fenêtre passe devant, ou celle qui y était bouge.
    ///
    /// Reprend la règle de la version Linux : la bulle se ferme si elle a assez vécu, et le maki
    /// descend d'un toit qui n'est plus le sien — mais il suit une même fenêtre qui se déplace.
    fn sur_fenetre(&mut self, mut f: crate::bureau::Fenetre) {
        // Du repère de l'écran (origine au coin de l'écran virtuel) vers le monde du maki (origine
        // au coin de la zone de travail). Les deux sont en pixels physiques, donc aucune échelle à
        // appliquer : c'est la raison pour laquelle le monde est mesuré ainsi.
        f.rect = (f.rect.0 - self.travail.left as f32, f.rect.1 - self.travail.top as f32, f.rect.2, f.rect.3);
        let autre = self.devant.as_ref().is_none_or(|g| g.classe != f.classe || g.titre != f.titre || g.rect != f.rect);
        if autre {
            self.fermer_bulle_auto();
        }
        if self.devant.as_ref().is_none_or(|g| g.classe != f.classe || g.titre != f.titre) {
            self.maki.quitter_toit();
        }
        crate::atspi::noter_fenetre(&f.titre, &f.classe);
        self.devant = Some(f);
    }

    /// Ferme la bulle si elle a assez vécu : un rappel ou une alerte reste plus longtemps qu'une
    /// simple remarque. Mêmes seuils que la version Linux, pour que la même chose disparaisse au
    /// même moment des deux côtés.
    fn fermer_bulle_auto(&mut self) {
        let Some(b) = &self.maki.bubble else { return };
        let min = if matches!(b.msg.kind, Kind::Reminder | Kind::Alert) { 3.0 } else { 1.0 };
        if b.age >= min {
            self.maki.bubble = None;
        }
    }

    /// Place la fenêtre pour que le maki reste visible : elle le suit, bornée par la zone de
    /// travail. Même principe que sous Wayland, où la surface est déplacée plutôt qu'agrandie.
    fn placer(&mut self) {
        let (w, h) = self.monde;
        let (sw, sh) = (self.largeur as f32, self.hauteur as f32);
        let (lo, hi) = self.cadre();

        // La fenêtre ne bouge que si le contenu approche de son bord. Sans cette règle elle serait
        // déplacée à chaque image — et chaque déplacement fait refaire la mise en page du bureau,
        // ce qui se voit. Marge de 16 px, comme dans la version Linux.
        let (ox, oy) = self.origin;
        let m = 16.0;
        // Après un agrandissement l'ancienne place peut faire déborder la surface sous le bas de
        // l'écran : Windows la remonterait d'office et tout serait dessiné trop haut.
        let dedans = ox + sw <= w + 0.5 && oy + sh <= h + 0.5;
        let tient = dedans
            && lo.0 >= ox + m
            && hi.0 <= ox + sw - m
            && lo.1 >= oy + m.min(lo.1.max(0.0))
            && hi.1 <= oy + sh - 2.0;
        if tient && self.place {
            return;
        }

        // Centré sur le contenu, le bas du contenu contre le bas de la surface : les pattes du
        // maki restent sur le sol, et la bulle trouve sa place au-dessus de sa tête.
        let cx = (lo.0 + hi.0) / 2.0;
        let sx = (cx - sw / 2.0).clamp(0.0, (w - sw).max(0.0)).round();
        let mut sy = (hi.1 - sh + 2.0).clamp(0.0, (h - sh).max(0.0));
        if lo.1 < sy {
            sy = lo.1.max(0.0);
        }
        self.origin = (sx, sy.round());
        self.place = true;
    }

    /// Dessine une image. Suit le même enchaînement que la version Linux : le maki sur un calque à
    /// part, avec son contour clair, puis composé sur l'image — c'est ce qui le détache d'un fond
    /// clair sans lui faire un halo carré.
    fn peindre(&mut self) {
        let s = self.s;
        let (sx, sy) = self.origin;
        let (mx, my) = self.maki.pos;
        let pose = self.maki.pose();

        self.pixmap.fill(tiny_skia::Color::TRANSPARENT);
        if self.invisible() {
            return;
        }

        if let Some(t) = &self.maki.tree {
            draw::tree(&mut self.pixmap, Transform::from_translate(t.x - sx, self.monde.1 - sy).pre_scale(s, s), t.grow, t.side, pose.time);
        }
        if let Some(v) = &self.maki.vine {
            draw::vine(&mut self.pixmap, v.x - sx, -sy, v.bottom - sy, v.grow, s, pose.time);
        }

        // Rectangle du maki dans la surface, selon sa pose : la même fonction que celle qui
        // décide si un clic l'a touché, pour que les deux ne puissent pas diverger.
        let r = self.rect_maki();

        let base = Transform::from_translate(mx - sx, my - sy).pre_scale(self.maki.dir * s, s);
        let lr = (r.0 - 10.0 * s, r.1 - 10.0 * s, r.2 + 20.0 * s, r.3 + 20.0 * s);
        let pad = 70.0 * s;
        let zone = (lr.0 - pad, lr.1 - pad, lr.2 + 2.0 * pad, lr.3 + 2.0 * pad);
        draw::clear_rect(&mut self.sprite, zone);
        draw::maki(&mut self.sprite, base, &pose);
        draw::outline(&mut self.sprite, lr, (1.6 * s).round().max(1.0) as i32, (236, 232, 222), 0.85);
        draw::blend_over(&mut self.pixmap, &self.sprite, zone, 1.0);
        draw::puffs(&mut self.pixmap, (sx, sy), 1.0, &self.maki.puffs);

        // La discussion, ou à défaut une bulle spontanée : jamais les deux à la fois. Leurs
        // tracés sont gardés pour savoir où sont leurs boutons au prochain clic.
        self.chat_layout = None;
        self.bulle_layout = None;
        let u = self.ue();
        if let Some((bx, by, au_dessus, _, _)) = self.boite_chat() {
            let tete = self.tete();
            let (hx, hy) = (tete.0 - sx, tete.1 - sy);
            // La pointe de la bulle part du haut du personnage : mêmes deux abscisses que la
            // version Linux, qui passe la tête pour `ax` et le haut du rectangle pour `ay`.
            let ay = if au_dessus { r.1 + 20.0 * s } else { hy };
            // Le curseur clignote : visible une demi-seconde sur deux, et seulement quand la
            // fenêtre tient vraiment le clavier.
            let caret = self.a_le_clavier && (pose.time * 2.0) as i64 % 2 == 0;
            self.chat_layout = Some(draw::chat_bubble(
                &mut self.pixmap, &self.texte, &self.chat, hx, ay, au_dessus, bx - sx, by - sy, u,
                caret, self.a_le_clavier, pose.time,
            ));
        } else if let Some(b) = &self.maki.bubble {
            let tete = self.tete();
            // Les boutons d'action ne sont pas câblés : `actions_for` vit dans `mascot/mod.rs` et
            // dépend de ses outils. La croix et le texte qui se révèle, eux, le sont.
            self.bulle_layout = Some(draw::fancy_bubble(
                &mut self.pixmap, &self.texte, &self.gras, b, &[], tete.0 - sx, tete.1 - sy, u,
            ));
        }
    }

    /// Envoie l'image à la fenêtre en calques.
    fn presenter(&mut self) {
        // RGBA prémultiplié (tiny-skia) → BGRA prémultiplié (ce que veut `UpdateLayeredWindow`
        // avec `AC_SRC_ALPHA`). Deux octets échangés par pixel, et l'alpha est déjà dans le bon
        // état : rien à convertir.
        let (l, h) = (self.largeur as usize, self.hauteur as usize);
        if self.bits.is_null() || l * h * 4 > self.pixmap.data().len() {
            return;
        }
        // SAFETY : `bits` pointe vers le bitmap créé à nos dimensions exactes, vivant tant que
        // `self.bit` l'est ; la source fait exactement la même taille.
        unsafe {
            let dst = std::slice::from_raw_parts_mut(self.bits, l * h * 4);
            let src = self.pixmap.data();
            for (d, p) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                d[0] = p[2];
                d[1] = p[1];
                d[2] = p[0];
                d[3] = p[3];
            }
        }

        let dst = POINT {
            x: self.travail.left + self.origin.0 as i32,
            y: self.travail.top + self.origin.1 as i32,
        };
        let taille = SIZE { cx: self.largeur as i32, cy: self.hauteur as i32 };
        let src = POINT { x: 0, y: 0 };
        let fusion = BLENDFUNCTION { operation: AC_SRC_OVER, drapeaux: 0, alpha: 255, format: AC_SRC_ALPHA };
        // SAFETY : tous les pointeurs sont les nôtres et vivent le temps de l'appel ; `self.dc`
        // porte le bitmap rempli ci-dessus.
        unsafe {
            UpdateLayeredWindow(self.fenetre, 0, &dst, &taille, self.dc, &src, 0, &fusion, ULW_ALPHA);
        }
    }

    /// Position du curseur dans le monde, et demande à être prévenu quand il part.
    fn souris(&mut self) {
        let mut p = POINT { x: 0, y: 0 };
        // SAFETY : écrit un POINT dans notre variable.
        if unsafe { GetCursorPos(&mut p) } == 0 {
            return;
        }
        let monde = (
            (p.x - self.travail.left) as f32,
            (p.y - self.travail.top) as f32,
        );
        self.pointeur = Some(monde);
        self.survole = true;
        if !self.entraine {
            self.entraine = true;
            let mut e = TRACKMOUSEEVENT {
                cbsize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                drapeaux: TME_LEAVE,
                fenetre: self.fenetre,
                survol: 0,
            };
            // SAFETY : structure remplie, avec la bonne taille et notre fenêtre.
            unsafe { TrackMouseEvent(&mut e) };
        }
        if self.traine {
            self.maki.drag_to(monde, MILLIS_PAR_IMAGE as f32 / 1000.0);
        }
    }

    fn prendre(&mut self) {
        if let Some(p) = self.pointeur {
            self.maki.grab(p);
            self.traine = true;
        }
    }

    fn poser(&mut self) {
        if self.traine {
            let ctx = self.ctx();
            self.maki.release(&ctx);
            self.traine = false;
        }
    }

    /// Taille du texte des bulles, échelle de l'écran comprise.
    ///
    /// La version Linux dessine en pixels logiques puis multiplie par le rapport d'échelle du
    /// moniteur. Ici le monde est déjà en pixels physiques, donc l'échelle doit entrer dans la
    /// taille du texte — sinon les bulles seraient moitié trop petites sur un écran à 200 %.
    fn ue(&self) -> f32 {
        self.cfg.maki.texte.clamp(0.8, 2.2) * self.echelle
    }

    /// Le rectangle du maki dans le monde, selon sa pose. Mêmes valeurs que la version Linux.
    ///
    /// Dans le monde et non dans la surface : c'est lui qui fixe l'endroit où placer la fenêtre, et cette
    /// décision ne peut pas dépendre de la place actuelle — ce serait tourner en rond.
    fn rect_monde(&self) -> R {
        let s = self.s;
        let (mx, my) = self.maki.pos;
        match self.maki.pose().kind {
            PoseKind::Dangle { .. } => (mx - 45.0 * s, my - 100.0 * s, 90.0 * s, 160.0 * s),
            PoseKind::Climb => (mx - 130.0 * s, my - 75.0 * s, 260.0 * s, 150.0 * s),
            PoseKind::Grimpe => (mx - 60.0 * s, my - 130.0 * s, 120.0 * s, 175.0 * s),
            PoseKind::Sleep => (mx - 70.0 * s, my - 45.0 * s, 130.0 * s, 48.0 * s),
            _ => (mx - 85.0 * s, my - 135.0 * s, 170.0 * s, 140.0 * s),
        }
    }

    /// Le même rectangle dans la surface : c'est celui que le clic teste et que le dessin utilise.
    ///
    /// Déduit du rectangle monde plutôt que recalculé, pour que le clic et l'image ne puissent
    /// jamais diverger — sinon on attraperait le maki en cliquant à côté de lui.
    fn rect_maki(&self) -> R {
        let (sx, sy) = self.origin;
        let r = self.rect_monde();
        (r.0 - sx, r.1 - sy, r.2, r.3)
    }

    /// Position de la tête selon la pose, dans le monde. Mêmes valeurs que la version Linux :
    /// c'est de là que part la pointe des bulles.
    fn tete(&self) -> (f32, f32) {
        let s = self.s;
        let (mx, my) = self.maki.pos;
        match self.maki.pose().kind {
            PoseKind::Sit => (mx + self.maki.dir * 14.0 * s, my - 88.0 * s),
            PoseKind::Dangle { .. } => (mx, my - 78.0 * s),
            PoseKind::Climb => (mx, my - 60.0 * s),
            PoseKind::Grimpe => (mx + self.maki.dir * 4.0 * s, my - 95.0 * s),
            PoseKind::Sleep => (mx + self.maki.dir * 24.0 * s, my - 30.0 * s),
            _ => (mx + self.maki.dir * 30.0 * s, my - 72.0 * s),
        }
    }

    /// Rien n'est dessiné cette image : caché par l'utilisateur, ou par une fenêtre en plein
    /// écran. Même règle que la version Linux — sauf qu'une discussion engagée ou une réponse en
    /// cours garde le maki visible : on ne fait pas disparaître quelqu'un à qui l'on parle.
    fn invisible(&self) -> bool {
        let plein_ecran = self.devant.as_ref().is_some_and(|f| f.plein_ecran);
        self.cache || (plein_ecran && !self.fige())
    }

    /// La boîte de la bulle de discussion, dans le monde : `(x, y, au-dessus, largeur, hauteur)`.
    ///
    /// Calculée avant le dessin, parce qu'elle fixe la taille de la surface et sa place : si elle
    /// n'était connue qu'au moment de dessiner, la bulle serait coupée par le bord de la fenêtre.
    /// Au-dessus de la tête s'il y a la place jusqu'en haut de l'écran, sinon dessous.
    fn boite_chat(&self) -> Option<(f32, f32, bool, f32, f32)> {
        if !self.chat.open || self.invisible() {
            return None;
        }
        let (s, u) = (self.s, self.ue());
        let (w, h) = self.monde;
        let bh = draw::chat_height(&self.texte, &self.chat, u) + 16.0 * u;
        let bw = 400.0 * u;
        // La bulle passe au-dessus du haut du personnage, oreilles comprises.
        let top = self.rect_monde().1 + 20.0 * s;
        let above = top - bh - 14.0 * u > 0.0;
        let suspendu = matches!(self.maki.pose().kind, PoseKind::Dangle { .. });
        let pieds = if suspendu { self.maki.pos.1 + 50.0 * s } else { self.maki.pos.1 + 8.0 * s };
        let by = if above { top - 6.0 * u } else { pieds.min(h - bh) };
        let bx = (self.tete().0 - bw * 0.45).clamp(6.0, (w - bw - 6.0).max(6.0));
        Some((bx, by, above, bw, bh))
    }

    /// Ce que l'image doit contenir, dans le monde : `(coin haut-gauche, coin bas-droite)`.
    ///
    /// Le maki, sa liane, son arbre et sa bulle. C'est cette boîte qui donne la taille de la
    /// surface et l'endroit où la poser — exactement comme dans la version Linux, dont les marges
    /// sont reprises ici.
    fn cadre(&self) -> ((f32, f32), (f32, f32)) {
        let (s, u) = (self.s, self.ue());
        let r = self.rect_monde();
        let mut lo = (r.0, r.1);
        let mut hi = (r.0 + r.2, r.1 + r.3);
        let mut grandir = |z: R| {
            lo = (lo.0.min(z.0), lo.1.min(z.1));
            hi = (hi.0.max(z.0 + z.2), hi.1.max(z.1 + z.3));
        };
        if let Some(v) = &self.maki.vine {
            grandir((v.x - 30.0 * s, 0.0, 60.0 * s, v.bottom * v.grow.max(0.01)));
        }
        if let Some(t) = &self.maki.tree {
            let hauteur = (Tree::height(s) + 70.0 * s) * t.grow.max(0.01);
            grandir((t.x - 140.0 * s, self.monde.1 - hauteur, 280.0 * s, hauteur));
        }
        if !self.invisible() {
            if let Some((bx, by, above, bw, bh)) = self.boite_chat() {
                grandir((bx, if above { by - bh } else { by }, bw, bh));
            } else if self.maki.bubble.is_some() {
                // Une bulle spontanée : sa place exacte n'est connue qu'au dessin, mais son
                // encombrement maximum l'est — même estimation que la version Linux.
                grandir(((self.tete().0 - 140.0 * u).max(0.0), r.1 + 20.0 * s - 250.0 * u, 420.0 * u, 250.0 * u));
            }
        }
        (lo, hi)
    }

    /// Taille de surface qu'il faut pour tout dessiner, sans jamais dépasser l'écran.
    ///
    /// Le plancher de 470×560 est celui de la version Linux : c'est la place qu'il faut au maki
    /// pour ses numéros. Le plafond est l'écran — une surface plus grande que lui ne servirait à
    /// rien et coûterait deux méga-octets par tranche.
    fn taille_voulue(&self) -> (u32, u32) {
        let s = self.s;
        let (lo, hi) = self.cadre();
        let voulu = (
            ((hi.0 - lo.0) + 24.0).max(470.0 * s).min(self.monde.0),
            ((hi.1 - lo.1) + 24.0).max(560.0 * s).min(self.monde.1),
        );
        (
            voulu.0.ceil().max(64.0) as u32,
            voulu.1.ceil().max(64.0) as u32,
        )
    }

    /// Ajuste la surface, le bitmap et la fenêtre à ce qu'il faut dessiner.
    ///
    /// Une fenêtre en calques laisse passer la souris là où l'alpha est nul : agrandir la surface
    /// ne bloque donc rien sous elle, ce qui permet de la faire varier librement.
    ///
    /// La marge de 200 px avant de rétrécir est la même que sous Wayland. Sans elle, une réponse
    /// qui s'écrit ligne à ligne ferait trembler la fenêtre à chaque image — et chaque changement
    /// de taille coûte une allocation GDI.
    fn ajuster(&mut self) {
        let (w, h) = self.taille_voulue();
        let grandir = w > self.largeur || h > self.hauteur;
        let retrecir = self.largeur > w + 200 || self.hauteur > h + 200;
        if !grandir && !retrecir {
            return;
        }
        let (Some(pm), Some(sp)) = (Pixmap::new(w, h), Pixmap::new(w, h)) else { return };
        let Ok((dc, bit, bits)) = dib(w, h) else {
            // L'ancienne taille reste : une bulle coupée vaut mieux qu'une fenêtre morte.
            return;
        };
        let (ancien_dc, ancien_bit) = (self.dc, self.bit);
        self.dc = dc;
        self.bit = bit;
        self.bits = bits;
        self.pixmap = pm;
        self.sprite = sp;
        self.largeur = w;
        self.hauteur = h;
        // SAFETY : le DC et le bitmap remplacés sont les nôtres et ne servent plus. Le DC d'abord :
        // un bitmap encore sélectionné dans un DC refuse de se supprimer. Puis la fenêtre, sans la
        // déplacer ni changer son ordre — sa position vient de `placer`, image par image.
        unsafe {
            DeleteDC(ancien_dc);
            DeleteObject(ancien_bit);
            SetWindowPos(self.fenetre, 0, 0, 0, w as i32, h as i32, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    /// Ouvre ou ferme la bulle de discussion.
    ///
    /// Jamais de clavier pris d'office : la bulle le reçoit quand on clique dedans et le rend dès
    /// qu'on clique ailleurs — c'est la règle de la version Linux, et elle vaut aussi ici.
    fn basculer_chat(&mut self) {
        self.chat.toggle();
        // Une bulle spontanée et la discussion ne se superposent jamais.
        self.maki.bubble = None;
        self.prendre_clavier(self.chat.open);
        if self.chat.open {
            self.maki.wave();
        }
    }

    /// Prend ou rend le clavier.
    ///
    /// La fenêtre est `NOACTIVATE` pour ne jamais voler le clavier de l'utilisateur : c'est une
    /// règle du projet, pas un détail. Mais une fenêtre qui ne peut pas être activée ne reçoit
    /// aucune touche. Pour taper dans la bulle il faut donc lever cet attribut le temps de la
    /// discussion, et le remettre dès qu'elle se ferme — d'où ce basculement, plutôt qu'une
    /// fenêtre activable en permanence.
    fn prendre_clavier(&mut self, veut: bool) {
        if self.a_le_clavier == veut || self.fenetre == 0 {
            return;
        }
        self.a_le_clavier = veut;
        // Appliquer après la fin du rappel courant : SetFocus envoie des messages synchrones.
        unsafe { PostMessageW(self.fenetre, WM_APPLIQUER_CLAVIER, usize::from(veut), 0) };
    }

    /// Un clic, dans les coordonnées de la surface.
    ///
    /// Même ordre que la version Linux : la discussion d'abord (elle est au-dessus), puis une bulle
    /// spontanée, puis le maki lui-même.
    fn clic(&mut self, p: (f32, f32)) {
        if let Some(lay) = self.chat_layout.clone() {
            if hit(lay.rect, p) {
                self.clic_chat(&lay, p);
                return;
            }
        }
        if let Some(lay) = self.bulle_layout.clone() {
            if hit(lay.rect, p) {
                self.clic_bulle(&lay, p);
                return;
            }
        }
        if hit(self.rect_maki(), p) {
            self.basculer_chat();
        }
    }

    /// Un clic dans la bulle de discussion. Même ordre de priorité que la version Linux : les
    /// boutons les plus précis d'abord, la zone de saisie en dernier.
    fn clic_chat(&mut self, lay: &ChatLayout, p: (f32, f32)) {
        let shift = touche_enfoncee(VK_SHIFT);
        if lay.stop.is_some_and(|r| hit(r, p)) {
            self.chat.arreter();
        } else if lay.joindre.is_some_and(|r| hit(r, p)) {
            self.chat.joindre();
        } else if let Some((_, i)) = lay.pieces.iter().find(|(r, _)| hit(*r, p)) {
            self.chat.retirer_piece(*i);
        } else if let Some((_, a)) = lay.histo.iter().rev().find(|(r, _)| hit(*r, p)) {
            self.chat.histo_act(a.clone());
            // L'historique a un champ de recherche : sans le clavier on ne pourrait pas y taper.
            self.prendre_clavier(true);
        } else if let Some((_, c)) = lay.cotes.iter().find(|(r, _)| hit(*r, p)) {
            self.chat.basculer_cote(*c);
        } else if hit(lay.histo_btn, p) || (lay.fil.2 > 0.0 && hit(lay.fil, p)) {
            self.chat.basculer_histo();
            self.prendre_clavier(true);
        } else if let Some((_, a)) = lay.picks_panel.iter().find(|(r, _)| hit(*r, p)) {
            self.chat.settings_act(a.clone());
        } else if let Some((_, pk)) = lay.picks.iter().find(|(r, _)| hit(*r, p)) {
            self.chat.pick(pk.clone());
        } else if let Some((_, k)) = lay.presse.iter().find(|(r, _)| hit(*r, p)) {
            self.chat.action_presse(*k);
        } else if lay.mode.2 > 0.0 && hit(lay.mode, p) {
            self.chat.mode_suivant();
        } else if hit(lay.gear, p) {
            self.chat.histo = None;
            if self.chat.panneau.is_some() {
                self.chat.settings_act(crate::chat::Act::Fermer);
            } else {
                self.chat.open_settings(0);
            }
        } else if hit(lay.chip, p) && self.chat.histo.is_some() {
            self.chat.histo = None;
        } else if hit(lay.chip, p) && self.chat.panneau.is_some() {
            self.chat.settings_act(crate::chat::Act::Fermer);
        } else if hit(lay.chip, p) {
            let a = if self.chat.picker.is_some() { crate::chat::Pick::Close } else { crate::chat::Pick::Open };
            self.chat.pick(a);
        } else if hit(lay.close, p) {
            self.basculer_chat();
        } else if lay.yes.is_some_and(|r| hit(r, p)) {
            self.chat.answer(true);
        } else if lay.no.is_some_and(|r| hit(r, p)) {
            self.chat.answer(false);
            self.maki.set_attitude(super::behavior::Attitude::Boude, 2.4);
        } else if let Some((_, u)) = lay.links.iter().find(|(r, _)| hit(*r, p)) {
            let u = u.clone();
            self.ouvrir_adresse(&u);
        } else if let Some((_, t)) = lay.copy.iter().find(|(r, _)| hit(*r, p)) {
            let t = t.clone();
            let _ = crate::clipboard::copy(&t);
            self.chat.entries.push(crate::chat::Entry::Info(t!("Réponse copiée dans le presse-papiers.").into()));
        } else {
            // Un clic ailleurs dans la bulle, ou dans la saisie : le curseur va au caractère le
            // plus proche, et la fenêtre prend le clavier pour qu'on puisse taper.
            if hit(lay.input, p) {
                if let Some(i) = lay.input_x.iter().enumerate().min_by(|a, b| (a.1 - p.0).abs().total_cmp(&(b.1 - p.0).abs())).map(|(i, _)| i) {
                    self.chat.input.placer(lay.input_off + i, shift);
                }
            }
            self.prendre_clavier(true);
        }
    }

    /// Un clic sur une bulle spontanée : sa croix la ferme ; ailleurs, un premier clic révèle la
    /// suite du texte et le suivant la ferme. Même comportement que la version Linux.
    fn clic_bulle(&mut self, lay: &BubbleLayout, p: (f32, f32)) {
        if hit(lay.close, p) {
            self.maki.bubble = None;
        } else if let Some(b) = &mut self.maki.bubble {
            if b.revealed() < b.full() {
                b.age = 60.0;
            } else {
                self.maki.bubble = None;
            }
        }
    }

    /// Ouvre une adresse dans le navigateur par défaut.
    ///
    /// `hote::ouvrir`, qui est `ShellExecuteW` ici et `xdg-open` sous Linux : une seule
    /// déclaration pour tout le projet, et la même garde des deux côtés. Seules les adresses web
    /// passent — un lien trouvé dans une réponse n'a pas à pouvoir ouvrir autre chose.
    fn ouvrir_adresse(&mut self, u: &str) {
        if !(u.starts_with("https://") || u.starts_with("http://")) {
            return;
        }
        match crate::hote::ouvrir(std::ffi::OsStr::new(u)) {
            Ok(()) => self.chat.entries.push(crate::chat::Entry::Info(t!("Ouvert : {}", u))),
            Err(_) => self.chat.entries.push(crate::chat::Entry::Info(t!("Le navigateur n'a pas pu s'ouvrir.").into())),
        }
    }

    /// Une touche, quand la discussion est ouverte. Même table que la version Linux, traduite des
    /// codes de touches X11/Wayland vers les touches virtuelles de Windows.
    fn touche(&mut self, vk: usize) {
        if !self.chat.open {
            return;
        }
        let (ctrl, shift) = (touche_enfoncee(VK_CONTROL) && !touche_enfoncee(VK_MENU), touche_enfoncee(VK_SHIFT));
        let champ = self.chat.has_field_focus();
        match vk {
            VK_ESCAPE => {
                if champ { self.chat.field_escape() } else if self.chat.busy && !crate::controle::arrete() { self.chat.arreter() } else { self.basculer_chat() }
            }
            VK_RETURN => {
                if champ {
                    self.chat.field_enter();
                } else if self.chat.confirm.is_none() {
                    self.chat.submit();
                }
            }
            VK_BACK => {
                if champ { self.chat.field_backspace() } else { self.chat.input.retour_arriere(ctrl) }
            }
            VK_DELETE if !champ => self.chat.input.supprimer(ctrl),
            VK_LEFT if !champ => self.chat.input.gauche(ctrl, shift),
            VK_RIGHT if !champ => self.chat.input.droite(ctrl, shift),
            VK_HOME if !champ => self.chat.input.debut(shift),
            VK_END if !champ => self.chat.input.fin(shift),
            // Vers le haut : remonter dans l'historique. Même sens que la version Linux.
            VK_PRIOR | VK_UP if !champ => self.chat.scroll += 3,
            VK_NEXT | VK_DOWN if !champ => self.chat.scroll = self.chat.scroll.saturating_sub(3),
            VK_V if ctrl => {
                if let Some(o) = crate::clipboard::coller() {
                    let t = String::from_utf8_lossy(&o);
                    if champ { self.chat.field_input(&t) } else { self.chat.input.inserer(&t) }
                }
            }
            VK_A if ctrl && !champ => self.chat.input.tout_selectionner(),
            VK_C if ctrl && !champ => {
                if let Some(t) = self.chat.input.texte_selectionne() {
                    let _ = crate::clipboard::copy(&t);
                }
            }
            VK_X if ctrl && !champ => {
                if let Some(t) = self.chat.input.couper() {
                    let _ = crate::clipboard::copy(&t);
                }
            }
            VK_U if ctrl && !champ => {
                self.chat.input.prendre();
            }
            // Ctrl+O : joindre des fichiers, comme « Ouvrir » partout ailleurs.
            VK_O if ctrl && !champ && self.chat.histo.is_none() && self.chat.panneau.is_none() => {
                self.chat.joindre();
            }
            _ => {}
        }
    }

    /// Un caractère saisi.
    ///
    /// Windows envoie l'UTF-16 morceau par morceau : un caractère hors du plan de base — un émoji,
    /// une écriture rare — arrive en deux `WM_CHAR`, une moitié haute puis une moitié basse. Les
    /// garder séparées donnerait deux caractères de remplacement à la place d'un émoji.
    fn caractere(&mut self, u: u16) {
        if !self.chat.open || !self.a_le_clavier {
            return;
        }
        self.tampon_utf16.push(u);
        let complet = match self.tampon_utf16.as_slice() {
            [hi] if (0xD800..=0xDBFF).contains(hi) => false, // il manque la moitié basse
            _ => true,
        };
        if !complet {
            return;
        }
        let pris = std::mem::take(&mut self.tampon_utf16);
        let texte = String::from_utf16_lossy(&pris);
        // Les touches de contrôle arrivent aussi ici (Retour arrière, Échap…) et sont déjà
        // traitées par `touche` : les retaper ici les ferait agir deux fois.
        if texte.is_empty() || texte.chars().any(|c| c.is_control()) {
            return;
        }
        if self.chat.has_field_focus() {
            self.chat.field_input(&texte);
        } else {
            self.chat.input.inserer(&texte);
        }
    }

    /// Les signaux de la discussion. Mêmes règles que la version Linux.
    fn sonder(&mut self) {
        // Une réponse arrive : il se redresse et « parle ».
        if !self.chat.flux.is_empty() {
            self.maki.set_attitude(super::behavior::Attitude::Parle, 0.4);
        }
        for sig in self.chat.poll() {
            match sig {
                Signal::Answered(t) if !self.chat.open => {
                    let court: String = t.chars().take(220).collect();
                    let court = if t.chars().count() > 220 { format!("{court}…") } else { court };
                    self.montrer(Message::new(Kind::Answer, t!("Voilà !"), court));
                }
                Signal::Answered(_) => self.maki.react(Kind::Answer),
                // Une confirmation est demandée : la bulle s'ouvre pour la montrer.
                Signal::NeedsConfirm if !self.chat.open => self.basculer_chat(),
                Signal::ConfigChanged => self.recharger(),
                _ => {}
            }
        }
        // Fin d'une visioconférence, d'une présentation ou d'un plein écran : ce qui avait été mis
        // de côté s'affiche maintenant, en une seule bulle.
        let occupe = self.occupe();
        if self.etait_occupe && !occupe {
            self.liberer_attente(t!("Pendant que tu étais occupé·e"));
        }
        self.etait_occupe = occupe;
    }

    /// Durée d'une bulle : le réglage, plus un quart de seconde par mot. Une longue réponse se lit
    /// plus lentement qu'un bonjour — même formule que la version Linux.
    fn duree_bulle(&self, texte: &str) -> f32 {
        self.cfg.maki.bulle_s as f32 + texte.split_whitespace().count() as f32 * 0.25
    }

    /// Montre un message : dans la discussion si elle est ouverte, sinon dans une bulle.
    ///
    /// Les deux ne se dessinent jamais ensemble, la discussion recouvrirait la bulle. Sans ce
    /// détour, un message arrivé discussion ouverte serait simplement perdu.
    fn montrer(&mut self, m: Message) {
        if self.chat.open {
            let t = if m.title.is_empty() { m.text.clone() } else { format!("{} — {}", m.title, m.text) };
            self.chat.entries.push(crate::chat::Entry::Maki(t));
            self.maki.react(m.kind);
        } else {
            let secs = self.duree_bulle(&m.text);
            self.maki.show(m, secs);
        }
    }

    /// Remarque spontanée : soumise à la discrétion et à l'intervalle entre deux paroles.
    ///
    /// La version Linux y ajoute l'absence de l'utilisateur, détectée par le protocole d'inactivité
    /// du composeur. Windows la donnerait par `GetLastInputInfo` ; tant qu'elle n'est pas mesurée
    /// ici, une remarque peut tomber pendant que personne ne regarde — elle n'est pas perdue pour
    /// autant, puisque l'occupation, elle, est détectée et met de côté.
    fn spontane(&mut self, m: Message) {
        let ecart = Duration::from_secs(self.cfg.maki.parole_min as u64 * 60);
        if self.occupe() {
            self.mettre_de_cote(m);
            return;
        }
        // Déjà en train de parler, de répondre ou d'être traîné : on ne se coupe pas la parole.
        let pris = self.chat.open || self.maki.bubble.is_some() || self.traine;
        if !self.quiet && !self.cache && !pris && self.derniere_parole.is_none_or(|d| d.elapsed() >= ecart) {
            self.derniere_parole = Some(Instant::now());
            self.montrer(m);
        }
    }

    /// Garde un message pour plus tard, sans doublon et sans dépasser huit lignes.
    fn mettre_de_cote(&mut self, m: Message) {
        if !self.en_attente.iter().any(|x| x.title == m.title && x.text == m.text) {
            self.en_attente.push(m);
        }
        self.en_attente.truncate(8);
    }

    /// Ce qui attendait s'affiche en une seule bulle : quatre lignes, pas huit bulles d'affilée.
    fn liberer_attente(&mut self, titre: &str) {
        if self.en_attente.is_empty() {
            return;
        }
        let lignes: Vec<String> = self.en_attente.drain(..).map(|m| if m.title.is_empty() { m.text } else { m.title }).collect();
        let texte = lignes.iter().take(4).map(|l| format!("• {}", l.lines().next().unwrap_or(""))).collect::<Vec<_>>().join("\n");
        self.montrer(Message::new(Kind::Info, titre, texte));
    }

    /// Relit la configuration : un réglage a changé dans la discussion.
    ///
    /// La discussion garde sa propre copie de la configuration — c'est elle qui lit les modèles et
    /// les clés — donc elle doit être rafraîchie aussi, sinon un changement de modèle ne prendrait
    /// effet qu'au prochain lancement.
    ///
    /// Changer d'espèce ou de taille ne relance pas le processus, contrairement à la version Linux
    /// qui se fait renaître par `sh`. Ici tout tient dans cette fenêtre : la nouvelle échelle est
    /// appliquée sur place, et `ajuster` recrée la surface à l'image suivante. C'est plus simple,
    /// et cela évite un `sh` qui n'existe pas sous Windows.
    fn recharger(&mut self) {
        let cfg = Config::load();
        let change = cfg.maki.espece != self.cfg.maki.espece || (cfg.maki.taille - self.cfg.maki.taille).abs() > 0.01;
        self.chat.reload(cfg.clone());
        self.cfg = cfg;
        draw::set_theme(&self.cfg.maki.theme);
        let espece = Espece::parse(&self.cfg.maki.espece).unwrap_or(Espece::Catta);
        draw::set_species(espece);
        self.maki.sp = espece;
        self.echelle = rapport_echelle();
        self.s = (self.cfg.maki.taille * espece.pal().size).clamp(0.4, 3.5) * self.echelle;
        if change {
            // Il se présente sous sa nouvelle allure, puis se replace : la taille ayant changé, sa
            // position d'avant n'a plus de sens.
            let ctx = self.ctx();
            self.maki.say(espece.presentation(), 3.0);
            self.maki.teleport(&ctx);
        }
    }
}

/// Zone de travail du moniteur principal : l'écran moins la barre des tâches. C'est le monde du
/// maki, et le sol est son bas.
fn zone_de_travail() -> RECT {
    let mut r = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
    // En cas d'échec la valeur par défaut (1920×1080 à l'origine) reste : un monde vide ferait
    // diviser par zéro dans `placer` et ne donnerait aucune image.
    // SAFETY : écrit un RECT dans notre variable, qui vit jusqu'à la fin de l'appel.
    unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut RECT as *mut c_void, 0) };
    r
}

/// Rapport d'échelle de l'écran (1,0 à 96 ppp, 1,5 à 144…). Sans lui le maki serait minuscule sur
/// un écran haute densité.
fn rapport_echelle() -> f32 {
    // SAFETY : DC de l'écran, obtenu puis rendu ; `GetDeviceCaps` ne fait que lire.
    let dc = unsafe { GetDC(0) };
    if dc == 0 {
        return 1.0;
    }
    let ppp = unsafe { GetDeviceCaps(dc, LOGPIXELSX) };
    unsafe { ReleaseDC(0, dc) };
    if ppp <= 0 {
        return 1.0;
    }
    (ppp as f32 / 96.0).clamp(1.0, 3.0)
}

/// Le DC mémoire et le bitmap qui portent l'image, créés une fois : les recréer à chaque image
/// coûterait plus cher que de dessiner.
fn dib(largeur: u32, hauteur: u32) -> Result<(isize, isize, *mut u8), String> {
    // SAFETY : DC de l'écran, prêté le temps de créer un DC compatible, puis rendu.
    let ecran = unsafe { GetDC(0) };
    if ecran == 0 {
        return Err("aucun écran disponible".into());
    }
    // SAFETY : DC compatible avec l'écran.
    let dc = unsafe { CreateCompatibleDC(ecran) };
    unsafe { ReleaseDC(0, ecran) };
    if dc == 0 {
        return Err("le DC mémoire n'a pas pu être créé".into());
    }

    let mut entete = BITMAPINFOHEADER {
        taille: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        largeur: largeur as i32,
        // Hauteur négative : bitmap « top-down », dont la première ligne est le haut de l'image.
        // C'est ce que tiny-skia produit, et l'inverser retournerait le maki la tête en bas.
        hauteur: -(hauteur as i32),
        plans: 1,
        bits: 32,
        compression: BI_RGB,
        taille_image: 0,
        x_par_metre: 0,
        y_par_metre: 0,
        couleurs: 0,
        importantes: 0,
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    // SAFETY : l'en-tête décrit 32 bits sans compression, donc aucune table de couleurs n'est
    // attendue après lui ; `bits` reçoit le pointeur du tampon, alloué par Windows.
    let bit = unsafe { CreateDIBSection(dc, &mut entete, DIB_RGB_COLORS, &mut bits, 0, 0) };
    if bit == 0 || bits.is_null() {
        // SAFETY : le DC créé plus haut ne doit pas fuir.
        unsafe { DeleteDC(dc) };
        return Err("le bitmap de la fenêtre n'a pas pu être créé".into());
    }
    // SAFETY : le bitmap est à nous et vit autant que le DC.
    unsafe { SelectObject(dc, bit) };
    Ok((dc, bit, bits as *mut u8))
}

/// Coordonnées d'un message souris dans la surface, lues dans `lParam`.
///
/// La fenêtre a exactement la taille de la surface, donc les coordonnées client sont celles de
/// l'image : aucun décalage à connaître. Les deux moitiés sont signées sur 16 bits — un point au
///-delà du bord gauche ou haut arrive négatif, et le lire comme non signé le placerait vers
/// 65 000 pixels, donc hors de tout rectangle.
fn local(l: isize) -> (f32, f32) {
    let x = (l & 0xffff) as u16 as i16 as f32;
    let y = ((l >> 16) & 0xffff) as u16 as i16 as f32;
    (x, y)
}

/// Une touche virtuelle est-elle enfoncée en ce moment ?
///
/// `GetKeyState` rend un `i16` dont le bit haut veut dire « enfoncée ». Ce bit est le signe d'un
/// `i16`, donc `< 0` dit exactement la même chose que `& 0x8000` — lequel ne compile pas, 32 768
/// ne tenant pas dans un `i16`.
fn touche_enfoncee(vk: i32) -> bool {
    // SAFETY : lecture de l'état d'une touche, sans effet et sans ressource à libérer.
    // La comparaison est DANS le bloc : écrit `unsafe { … } < 0`, Rust lit `< 0` comme le début
    // d'une liste d'arguments de type, et non comme une comparaison.
    unsafe { GetKeyState(vk) < 0 }
}

fn large(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win32_alpha_and_focus_constants() {
        assert_eq!(ULW_ALPHA, 2);
        assert_eq!(WM_SETFOCUS, 7);
        assert_eq!(WM_KILLFOCUS, 8);
    }

    #[test]
    fn the_work_area_is_never_empty_or_inverted() {
        // Un monde de taille nulle ferait diviser par zéro dans `placer` et renverrait une image
        // vide. La valeur par défaut doit donc tenir même si Windows ne répond pas.
        let r = zone_de_travail();
        assert!(r.right > r.left, "zone de travail inversée : {r:?}");
        assert!(r.bottom > r.top, "zone de travail inversée : {r:?}");
    }

    #[test]
    fn the_scale_factor_stays_in_a_usable_range() {
        // Un rapport d'échelle aberrant (0, ou 40 sur un pilote qui ment) donnerait un maki
        // invisible ou une surface de plusieurs centaines de méga-octets.
        let e = rapport_echelle();
        assert!((1.0..=3.0).contains(&e), "rapport d'échelle hors bornes : {e}");
    }

    #[test]
    fn the_mascot_scale_is_clamped_like_on_linux() {
        // Même règle que la version Wayland : la taille demandée, pondérée par l'espèce, bornée.
        // Une valeur extrême dans config.toml ne doit pas pouvoir agrandir la surface sans limite.
        for demande in [0.0f32, 0.01, 1.0, 12.0, 1000.0] {
            let s = (demande * Espece::Catta.pal().size).clamp(0.4, 3.5);
            assert!((0.4..=3.5).contains(&s), "taille non bornée pour {demande}");
        }
    }

    #[test]
    fn premultiplied_rgba_swaps_to_bgra_without_touching_alpha() {
        // C'est le cœur de la présentation : `UpdateLayeredWindow` avec `AC_SRC_ALPHA` veut du
        // BGRA prémultiplié, et tiny-skia donne du RGBA prémultiplié. Seul l'ordre change, l'alpha
        // doit rester exactement où il est — le déplacer rendrait la fenêtre opaque ou invisible.
        let rgba: [u8; 8] = [10, 20, 30, 40, 0, 0, 0, 0];
        let mut bgra = [0u8; 8];
        for (d, p) in bgra.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
            d[0] = p[2];
            d[1] = p[1];
            d[2] = p[0];
            d[3] = p[3];
        }
        assert_eq!(bgra, [30, 20, 10, 40, 0, 0, 0, 0]);
    }
}
