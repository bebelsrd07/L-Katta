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
//! La surface fait 470×560 pixels logiques, comme sous Wayland, multipliés par le rapport
//! d'échelle de l'écran. En 100 % cela donne un peu plus d'un méga-octet pour l'image et autant
//! pour le bitmap système — très en dessous du budget de 15 Mo. La fenêtre est déplacée pour
//! suivre le maki plutôt que d'occuper tout l'écran, ce qui est aussi ce que font les backends
//! Wayland et X11.
//!
//! ## Ce qui n'est pas encore câblé
//!
//! La bulle de discussion, les réglages, le clavier et l'enchaînement vers `brain` existent dans
//! `draw` et `chat`, qui sont portables, mais ne sont pas branchés ici : voir `mod.rs`. Ce
//! module affiche le maki, l'anime, le fait réagir aux messages du démon et le laisse être
//! traîné à la souris.

use std::cell::RefCell;
use std::ffi::c_void;
use std::time::Instant;

use tiny_skia::{Pixmap, Transform};

use super::behavior::{Ctx, Exits, Maki, PoseKind};
use super::draw::{self, Text};
use super::especes::Espece;
use crate::config::Config;
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
const WM_MOUSELEAVE: u32 = 0x02A3;

const LOGPIXELSX: i32 = 88;
const SPI_GETWORKAREA: u32 = 0x0030;
const DIB_RGB_COLORS: u32 = 0;
const BI_RGB: u32 = 0;
const ULW_ALPHA: u32 = 0x0000_0004;
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

    let etat = Etat {
        cfg,
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
        pointeur: None,
        survole: false,
        entraine: false,
        traine: false,
        quiet: false,
        cache: false,
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
                DeleteObject(a.bit);
                DeleteDC(a.dc);
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
            avec_etat(|a| a.souris());
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
            avec_etat(|a| a.prendre());
            0
        }
        WM_LBUTTONUP => {
            // SAFETY : fin de la capture prise à WM_LBUTTONDOWN.
            unsafe { ReleaseCapture() };
            avec_etat(|a| a.poser());
            0
        }
        WM_RBUTTONUP => {
            // Clic droit : il salue. Le menu contextuel et la bulle de discussion viennent avec
            // le clavier, pas encore câblés ici (voir `mod.rs`).
            avec_etat(|a| a.maki.wave());
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
        if let Some(a) = &mut *e.borrow_mut() {
            f(a);
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
    pointeur: Option<(f32, f32)>,
    survole: bool,
    entraine: bool,
    traine: bool,
    quiet: bool,
    cache: bool,
    derniere: Instant,
}

impl Etat {
    /// Une image : lire les messages du démon, avancer le maki, dessiner, afficher.
    fn tic(&mut self) {
        let maintenant = Instant::now();
        let dt = maintenant.duration_since(self.derniere).as_secs_f32().clamp(0.0, 0.1);
        self.derniere = maintenant;

        self.lire_le_demon();
        let ctx = self.ctx();
        self.maki.step(dt, ctx);
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
            Msg::Say(t) => self.maki.say(t, 6.0),
            Msg::Reminder(t) => self.maki.say(t, 8.0),
            // Une remarque spontanée est soumise à la discrétion : en mode silencieux elle tombe.
            Msg::Hint(t) => {
                if !self.quiet {
                    self.maki.say(t, 5.0);
                }
            }
            Msg::Bubble(msg, toujours) => {
                if toujours || !self.quiet {
                    self.maki.show(msg, 7.0);
                }
            }
            Msg::Flip => self.maki.flip(),
            Msg::Wave => self.maki.wave(),
            Msg::Tree => self.maki.tree_show(&ctx),
            Msg::Teleport => self.maki.teleport(&ctx),
            Msg::Climb => self.maki.climb_edge(&ctx),
            Msg::Quiet(v) => self.quiet = v.unwrap_or(!self.quiet),
            Msg::Hide(v) => self.cache = v.unwrap_or(!self.cache),
            Msg::Reload => {
                self.cfg = Config::load();
                draw::set_theme(&self.cfg.maki.theme);
            }
            Msg::Quit => {
                // SAFETY : détruit notre fenêtre, ce qui fait envoyer WM_DESTROY puis quitter la
                // boucle de messages.
                unsafe { DestroyWindow(self.fenetre) };
            }
            // Non câblés ici, et c'est écrit plutôt que laissé au hasard :
            // `Chat` et `Raccourci` ouvrent la bulle de discussion, qui demande le clavier ;
            // `Fenetre`, `Occupe`, `ActiveOutput`, `Screen` et `Desktop` viennent du suivi du
            // bureau, absent sous Windows pour l'instant ; `Capture` sert aux essais X11.
            Msg::Chat | Msg::Raccourci | Msg::Fenetre(_) | Msg::Occupe { .. } | Msg::ActiveOutput(_) | Msg::Screen | Msg::Desktop(_) | Msg::Capture(_) => {}
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
            quiet: self.quiet,
            acrobatics: self.cfg.maki.acrobaties,
            show_gap_min: self.cfg.maki.numeros_min.max(1) as f32,
            hovered: self.survole,
            pointer: self.pointeur,
            // Pas encore de bulle de discussion : rien ne le fige sur place.
            chatting: self.traine,
            thinking: false,
            bubble_hover: false,
            band: if self.cfg.maki.zone == "libre" { 0.0 } else { 0.17 },
            // Un seul moniteur pris en compte : aucune sortie voisine, donc il ne traverse pas.
            exits: Exits { left: false, right: false, up: false, down: false },
            // Le suivi de la fenêtre active n'est pas câblé sous Windows (`bureau::demarrer` n'y
            // fait rien) : rien à éviter, rien où monter. Le dire ici plutôt que de laisser croire
            // que le maki a choisi de rester au sol.
            avoid: None,
            toit: None,
        }
    }

    /// Place la fenêtre pour que le maki reste visible : elle le suit, bornée par la zone de
    /// travail. Même principe que sous Wayland, où la surface est déplacée plutôt qu'agrandie.
    fn placer(&mut self) {
        let (w, h) = (self.largeur as f32, self.hauteur as f32);
        let (mx, my) = self.maki.pos;
        // Le maki est dessiné vers le bas de la surface : on aligne la surface sur le sol.
        let ox = (mx - w * 0.5).clamp(0.0, (self.monde.0 - w).max(0.0));
        let oy = (my - h * 0.72).clamp(0.0, (self.monde.1 - h).max(0.0));
        self.origin = (ox, oy);
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
        if self.cache {
            return;
        }

        if let Some(t) = &self.maki.tree {
            draw::tree(&mut self.pixmap, Transform::from_translate(t.x - sx, self.monde.1 - sy).pre_scale(s, s), t.grow, t.side, pose.time);
        }
        if let Some(v) = &self.maki.vine {
            draw::vine(&mut self.pixmap, v.x - sx, -sy, v.bottom - sy, v.grow, s, pose.time);
        }

        // Rectangle du maki dans le monde, selon sa pose : mêmes valeurs que sous Linux, à
        // l'échelle près.
        let r: (f32, f32, f32, f32) = match pose.kind {
            PoseKind::Dangle { .. } => (mx - 45.0 * s, my - 100.0 * s, 90.0 * s, 160.0 * s),
            PoseKind::Climb => (mx - 130.0 * s, my - 75.0 * s, 260.0 * s, 150.0 * s),
            PoseKind::Grimpe => (mx - 60.0 * s, my - 130.0 * s, 120.0 * s, 175.0 * s),
            PoseKind::Sleep => (mx - 70.0 * s, my - 45.0 * s, 130.0 * s, 48.0 * s),
            _ => (mx - 85.0 * s, my - 135.0 * s, 170.0 * s, 140.0 * s),
        };

        let base = Transform::from_translate(mx - sx, my - sy).pre_scale(self.maki.dir * s, s);
        let lr = (r.0 - sx - 10.0 * s, r.1 - sy - 10.0 * s, r.2 + 20.0 * s, r.3 + 20.0 * s);
        let pad = 70.0 * s;
        let zone = (lr.0 - pad, lr.1 - pad, lr.2 + 2.0 * pad, lr.3 + 2.0 * pad);
        draw::clear_rect(&mut self.sprite, zone);
        draw::maki(&mut self.sprite, base, &pose);
        draw::outline(&mut self.sprite, lr, (1.6 * s).round().max(1.0) as i32, (236, 232, 222), 0.85);
        draw::blend_over(&mut self.pixmap, &self.sprite, zone, 1.0);
        draw::puffs(&mut self.pixmap, (sx, sy), 1.0, &self.maki.puffs);

        // Une bulle spontanée : au-dessus de sa tête. Les boutons d'action ne sont pas encore
        // câblés — il faudrait la souris sur des zones, donc `draw::hit`, ce qui vient avec la
        // bulle de discussion.
        if let Some(b) = &self.maki.bubble {
            let u = self.cfg.maki.texte.clamp(0.8, 2.2);
            let tete = (mx - sx, my - sy - 88.0 * s);
            draw::fancy_bubble(&mut self.pixmap, &self.texte, &self.gras, b, &[], tete.0, tete.1, u);
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
}

/// Zone de travail du moniteur principal : l'écran moins la barre des tâches. C'est le monde du
/// maki, et le sol est son bas.
fn zone_de_travail() -> RECT {
    let mut r = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
    // SAFETY : écrit un RECT dans notre variable ; en cas d'échec la valeur par défaut reste.
    // En cas d'échec la valeur par défaut (1920×1080 à l'origine) reste : un monde vide ferait
    // diviser par zéro dans `placer` et ne donnerait aucune image.
    // SAFETY : écrit un RECT dans notre variable.
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

fn large(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
