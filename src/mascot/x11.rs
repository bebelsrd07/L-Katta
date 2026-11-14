//! Affichage X11 (prototype) : le maki dans une fenêtre ARGB sans décoration, au-dessus de
//! tout (override-redirect), qui ne reçoit les clics que sur lui (extension SHAPE). Même
//! comportement (`behavior.rs`) et même dessin (`draw.rs`) que sous Wayland.
//!
//! `lkatta x11-essai [--capture image.png] [--duree s]` : essai autonome (le maki complet,
//! bulles et discussion, viendra quand `mod.rs` sera séparé en scène + affichage ; voir
//! docs/TODO.md).

use std::time::{Duration, Instant};

use tiny_skia::{Pixmap, Transform};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::wrapper::ConnectionExt as _;
#[allow(unused_imports)]
use x11rb::connection::RequestConnection as _;

use super::behavior::{Ctx, Exits, Maki, Mode};
use super::draw;

/// Taille de la fenêtre du maki (px).
const LARGEUR: u16 = 260;
const HAUTEUR: u16 = 280;

/// Visuel 32 bits (ARGB) de l'écran : la transparence passe par lui.
fn visuel_argb(ecran: &xproto::Screen) -> Option<xproto::Visualid> {
    ecran
        .allowed_depths
        .iter()
        .filter(|d| d.depth == 32)
        .flat_map(|d| d.visuals.iter())
        .find(|v| v.class == xproto::VisualClass::TRUE_COLOR && v.red_mask == 0x00ff_0000)
        .map(|v| v.visual_id)
}

/// RGBA prémultiplié (tiny-skia) → BGRA prémultiplié (X11 32 bits, petit-boutiste).
pub fn vers_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut v = rgba.to_vec();
    for p in v.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    v
}

/// Rectangle d'entrée (zone cliquable) : le maki seulement, le reste laisse passer les clics.
fn zone_entree(c: &impl Connection, w: xproto::Window, r: (i16, i16, u16, u16)) -> Result<(), String> {
    c.shape_rectangles(shape::SO::SET, shape::SK::INPUT, xproto::ClipOrdering::UNSORTED, w, 0, 0, &[xproto::Rectangle { x: r.0, y: r.1, width: r.2, height: r.3 }])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn essai(capture: Option<&str>, duree: f32) -> Result<(), String> {
    let (c, n) = x11rb::connect(None).map_err(|e| t!("pas de serveur X ({})", e))?;
    let ecran = c.setup().roots[n].clone();
    let visuel = visuel_argb(&ecran).ok_or(t!("le serveur X n'a pas de visuel 32 bits (transparence impossible)"))?;
    c.shape_query_version().map_err(|e| e.to_string())?.reply().map_err(|_| t!("le serveur X n'a pas l'extension SHAPE"))?;
    let carte = c.generate_id().map_err(|e| e.to_string())?;
    c.create_colormap(xproto::ColormapAlloc::NONE, carte, ecran.root, visuel).map_err(|e| e.to_string())?;
    let w = c.generate_id().map_err(|e| e.to_string())?;
    let (x, y) = (ecran.width_in_pixels.saturating_sub(LARGEUR + 40) as i16, ecran.height_in_pixels.saturating_sub(HAUTEUR + 40) as i16);
    let attrs = xproto::CreateWindowAux::new()
        .background_pixel(0)
        .border_pixel(0)
        .colormap(carte)
        .override_redirect(1)
        .event_mask(xproto::EventMask::BUTTON_PRESS | xproto::EventMask::EXPOSURE);
    c.create_window(32, w, ecran.root, x, y, LARGEUR, HAUTEUR, 0, xproto::WindowClass::INPUT_OUTPUT, visuel, &attrs).map_err(|e| e.to_string())?;
    // Nom et rôle, pour les gestionnaires de fenêtres qui les lisent.
    c.change_property8(xproto::PropMode::REPLACE, w, xproto::AtomEnum::WM_NAME, xproto::AtomEnum::STRING, b"L Katta").map_err(|e| e.to_string())?;
    c.change_property8(xproto::PropMode::REPLACE, w, xproto::AtomEnum::WM_CLASS, xproto::AtomEnum::STRING, b"lkatta\0lkatta\0").map_err(|e| e.to_string())?;
    let gc = c.generate_id().map_err(|e| e.to_string())?;
    c.create_gc(gc, w, &xproto::CreateGCAux::new()).map_err(|e| e.to_string())?;
    c.map_window(w).map_err(|e| e.to_string())?;
    c.flush().map_err(|e| e.to_string())?;

    let mut maki = Maki::new(LARGEUR as f32 / 2.0, 1.0, 11);
    maki.mode = Mode::Sit;
    let sol = HAUTEUR as f32 - 12.0;
    maki.pos = (LARGEUR as f32 / 2.0, sol);
    let ctx = Ctx {
        w: LARGEUR as f32,
        h: sol,
        night: false,
        quiet: true,
        acrobatics: false,
        show_gap_min: 999.0,
        hovered: false,
        pointer: None,
        chatting: true,
        thinking: false,
        bubble_hover: false,
        day: true,
        band: 0.0,
        exits: Exits::default(),
        avoid: None,
        toit: None,
    };
    let mut pm = Pixmap::new(LARGEUR as u32, HAUTEUR as u32).ok_or("image")?;
    let debut = Instant::now();
    let mut dernier = Instant::now();
    let mut clics = 0;
    loop {
        while let Some(ev) = c.poll_for_event().map_err(|e| e.to_string())? {
            if let Event::ButtonPress(_) = ev {
                clics += 1;
                maki.wave();
            }
        }
        let dt = dernier.elapsed().as_secs_f32();
        dernier = Instant::now();
        maki.step(dt, ctx);
        maki.pos = (LARGEUR as f32 / 2.0, sol);
        if !matches!(maki.mode, Mode::Sit | Mode::Wave | Mode::Think | Mode::Stretch | Mode::Scratch) {
            maki.mode = Mode::Sit;
        }
        pm.fill(tiny_skia::Color::TRANSPARENT);
        let pose = maki.pose();
        draw::maki(&mut pm, Transform::from_translate(maki.pos.0, maki.pos.1).pre_scale(1.2, 1.2), &pose);
        c.put_image(xproto::ImageFormat::Z_PIXMAP, w, gc, LARGEUR, HAUTEUR, 0, 0, 0, 32, &vers_bgra(pm.data())).map_err(|e| e.to_string())?;
        // Seul le maki reçoit les clics (zone recalculée : il peut se lever, saluer…).
        zone_entree(&c, w, ((LARGEUR / 2 - 70) as i16, (HAUTEUR - 170) as i16, 140, 165))?;
        // Toujours au-dessus (un gestionnaire peut remonter une autre fenêtre).
        c.configure_window(w, &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE)).map_err(|e| e.to_string())?;
        c.flush().map_err(|e| e.to_string())?;
        let t = debut.elapsed().as_secs_f32();
        if t >= duree {
            break;
        }
        std::thread::sleep(Duration::from_millis(33));
    }
    if let Some(f) = capture {
        // Image de notre fenêtre (avant composition) et état vu par le serveur.
        let img = c.get_image(xproto::ImageFormat::Z_PIXMAP, w, 0, 0, LARGEUR, HAUTEUR, !0).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        let mut rgba = vers_bgra(&img.data);
        let opaques = rgba.chunks_exact(4).filter(|p| p[3] > 0).count();
        let mut out = Pixmap::new(LARGEUR as u32, HAUTEUR as u32).ok_or("image")?;
        out.data_mut().copy_from_slice(&rgba.drain(..).collect::<Vec<u8>>());
        out.save_png(f).map_err(|e| e.to_string())?;
        let a = c.get_window_attributes(w).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        let entree = c.shape_get_rectangles(w, shape::SK::INPUT).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        println!(
            "fenêtre X11 : visible={:?}, au-dessus (override-redirect)={}, pixels du maki={}, zone cliquable={:?}, clics={}",
            a.map_state,
            a.override_redirect,
            opaques,
            entree.rectangles.iter().map(|r| (r.x, r.y, r.width, r.height)).collect::<Vec<_>>(),
            clics
        );
    }
    c.destroy_window(w).map_err(|e| e.to_string())?;
    c.flush().map_err(|e| e.to_string())?;
    Ok(())
}

// --- affichage X11 du vrai maki ---------------------------------------------------------------

/// Affichage X11 : une fenêtre ARGB au-dessus de tout, qui suit le contenu (maki, bulles).
pub struct X11Aff {
    c: x11rb::rust_connection::RustConnection,
    win: xproto::Window,
    gc: xproto::Gcontext,
    taille: (u32, u32),
    pos: (i32, i32),
    zones: Vec<(i32, i32, i32, i32)>,
    /// La position doit être (re)donnée au serveur (nouvel écran).
    pub(super) a_placer: bool,
    /// Table des touches : (premier code, symboles par touche, symboles).
    touches: (u8, u8, Vec<u32>),
}

impl X11Aff {
    pub(super) fn redimensionner(&mut self, w: u32, h: u32) {
        if self.taille != (w, h) {
            self.taille = (w, h);
            let _ = self.c.configure_window(self.win, &xproto::ConfigureWindowAux::new().width(w).height(h));
        }
    }

    pub(super) fn deplacer(&mut self, x: i32, y: i32) {
        if self.pos != (x, y) || self.a_placer {
            self.pos = (x, y);
            self.a_placer = false;
            let _ = self.c.configure_window(self.win, &xproto::ConfigureWindowAux::new().x(x).y(y).stack_mode(xproto::StackMode::ABOVE));
        }
    }

    pub(super) fn zones(&mut self, z: &[(i32, i32, i32, i32)]) {
        if self.zones == z {
            return;
        }
        self.zones = z.to_vec();
        let r: Vec<xproto::Rectangle> = z.iter().map(|z| xproto::Rectangle { x: z.0 as i16, y: z.1 as i16, width: z.2.max(0) as u16, height: z.3.max(0) as u16 }).collect();
        let _ = self.c.shape_rectangles(shape::SO::SET, shape::SK::INPUT, xproto::ClipOrdering::UNSORTED, self.win, 0, 0, &r);
    }

    pub(super) fn presenter(&mut self, pm: &Pixmap) {
        // Essais : LKATTA_X11_CAPTURE=fichier.png garde la dernière image envoyée.
        if let Some(f) = std::env::var_os("LKATTA_X11_CAPTURE") {
            let _ = pm.save_png(f);
        }
        let (w, h) = (pm.width() as u16, pm.height() as u16);
        let _ = self.c.put_image(xproto::ImageFormat::Z_PIXMAP, self.win, self.gc, w, h, 0, 0, 0, 32, &vers_bgra(pm.data()));
        let _ = self.c.flush();
    }

    /// La bulle prend le clavier (clic dans la bulle) ou le rend au reste du bureau.
    pub(super) fn clavier(&mut self, oui: bool) {
        let cible = if oui { self.win } else { u32::from(xproto::InputFocus::POINTER_ROOT) };
        let _ = self.c.set_input_focus(xproto::InputFocus::POINTER_ROOT, cible, x11rb::CURRENT_TIME);
        let _ = self.c.flush();
    }

    /// Symbole d'une touche (avec Maj ou Verr. maj : le deuxième symbole de la touche).
    fn symbole(&self, code: u8, maj: bool) -> u32 {
        let (premier, par, ref t) = self.touches;
        let i = (code.saturating_sub(premier) as usize) * par as usize;
        let base = t.get(i).copied().unwrap_or(0);
        let haut = t.get(i + 1).copied().unwrap_or(0);
        if maj && haut != 0 { haut } else { base }
    }
}

/// Écrans X11 (RandR « moniteurs » : un par écran physique, nom et géométrie).
fn ecrans(c: &impl Connection, racine: xproto::Window, larg: u16, haut: u16) -> Vec<(String, (f32, f32, f32, f32))> {
    use x11rb::protocol::randr::ConnectionExt as _;
    let mut v = vec![];
    if let Ok(Ok(r)) = c.randr_get_monitors(racine, true).map(|r| r.reply()) {
        for m in r.monitors {
            let nom = c.get_atom_name(m.name).ok().and_then(|r| r.reply().ok()).map(|n| String::from_utf8_lossy(&n.name).into_owned()).unwrap_or_default();
            v.push((nom, (m.x as f32, m.y as f32, m.width as f32, m.height as f32)));
        }
    }
    if v.is_empty() {
        v.push(("X11".into(), (0.0, 0.0, larg as f32, haut as f32)));
    }
    v
}

/// Lance le maki sur un serveur X11 (sessions XFCE, Cinnamon, MATE, i3, KDE en X11…).
pub fn run(cfg: crate::config::Config, rx: smithay_client_toolkit::reexports::calloop::channel::Channel<crate::ipc::Msg>) -> Result<(), String> {
    use smithay_client_toolkit::reexports::calloop::{self, EventLoop, generic::Generic, Interest, Mode as CMode, PostAction, timer::{TimeoutAction, Timer}};
    use std::os::fd::AsFd;
    let (c, n) = x11rb::connect(None).map_err(|e| t!("pas de serveur X ({})", e))?;
    let ecran = c.setup().roots[n].clone();
    let visuel = visuel_argb(&ecran).ok_or(t!("le serveur X n'a pas de visuel 32 bits (transparence impossible)"))?;
    c.shape_query_version().map_err(|e| e.to_string())?.reply().map_err(|_| t!("le serveur X n'a pas l'extension SHAPE"))?;
    let carte = c.generate_id().map_err(|e| e.to_string())?;
    c.create_colormap(xproto::ColormapAlloc::NONE, carte, ecran.root, visuel).map_err(|e| e.to_string())?;

    draw::set_theme(&cfg.maki.theme);
    let espece = super::especes::Espece::parse(&cfg.maki.espece).unwrap_or_default();
    draw::set_species(espece);
    let s = (cfg.maki.taille * espece.pal().size).clamp(0.4, 3.5);
    let (sw, sh) = ((470.0 * s) as u32, (560.0 * s) as u32);
    let w = c.generate_id().map_err(|e| e.to_string())?;
    let masque = xproto::EventMask::BUTTON_PRESS
        | xproto::EventMask::BUTTON_RELEASE
        | xproto::EventMask::POINTER_MOTION
        | xproto::EventMask::ENTER_WINDOW
        | xproto::EventMask::LEAVE_WINDOW
        | xproto::EventMask::KEY_PRESS
        | xproto::EventMask::FOCUS_CHANGE
        | xproto::EventMask::EXPOSURE;
    let attrs = xproto::CreateWindowAux::new().background_pixel(0).border_pixel(0).colormap(carte).override_redirect(1).event_mask(masque);
    c.create_window(32, w, ecran.root, 0, 0, sw as u16, sh as u16, 0, xproto::WindowClass::INPUT_OUTPUT, visuel, &attrs).map_err(|e| e.to_string())?;
    c.change_property8(xproto::PropMode::REPLACE, w, xproto::AtomEnum::WM_NAME, xproto::AtomEnum::STRING, b"L Katta").map_err(|e| e.to_string())?;
    c.change_property8(xproto::PropMode::REPLACE, w, xproto::AtomEnum::WM_CLASS, xproto::AtomEnum::STRING, b"lkatta\0lkatta\0").map_err(|e| e.to_string())?;
    let gc = c.generate_id().map_err(|e| e.to_string())?;
    c.create_gc(gc, w, &xproto::CreateGCAux::new()).map_err(|e| e.to_string())?;
    // Rien de cliquable tant que rien n'est dessiné.
    c.shape_rectangles(shape::SO::SET, shape::SK::INPUT, xproto::ClipOrdering::UNSORTED, w, 0, 0, &[]).map_err(|e| e.to_string())?;
    c.map_window(w).map_err(|e| e.to_string())?;
    let setup = c.setup();
    let (premier, dernier) = (setup.min_keycode, setup.max_keycode);
    let touches = c
        .get_keyboard_mapping(premier, dernier - premier + 1)
        .map_err(|e| e.to_string())?
        .reply()
        .map(|r| (premier, r.keysyms_per_keycode, r.keysyms))
        .unwrap_or((premier, 1, vec![]));
    c.flush().map_err(|e| e.to_string())?;
    let liste = ecrans(&c, ecran.root, ecran.width_in_pixels, ecran.height_in_pixels);
    // Le descripteur de la connexion, pour réveiller la boucle quand X parle.
    let fd = c.stream().as_fd().try_clone_to_owned().map_err(|e| e.to_string())?;
    let aff = X11Aff { c, win: w, gc, taille: (sw, sh), pos: (0, 0), zones: vec![], a_placer: true, touches };

    let mut boucle: EventLoop<'static, super::App> = EventLoop::try_new().map_err(|e| e.to_string())?;
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7);
    let mut app = super::App::nouvelle(cfg, None, Some(aff), boucle.handle(), (sw, sh, s), espece, seed)?;
    for (i, (nom, g)) in liste.into_iter().enumerate() {
        app.screens.push(super::Screen { out: super::Sortie::X(i as u32), probe: None, area: Some((g.2, g.3)), geom: Some(g), nom });
    }
    app.current = app.screens.first().map(|s| s.out.clone());
    app.area = app.screens.first().and_then(|s| s.area);
    app.configured = true;
    app.configured_once = false;

    boucle
        .handle()
        .insert_source(rx, |ev, _, app: &mut super::App| {
            if let calloop::channel::Event::Msg(m) = ev {
                app.on_msg(m);
            }
        })
        .map_err(|e| e.to_string())?;
    boucle
        .handle()
        .insert_source(Timer::from_duration(Duration::from_millis(50)), |_, _, app: &mut super::App| TimeoutAction::ToDuration(app.tick()))
        .map_err(|e| e.to_string())?;
    boucle
        .handle()
        .insert_source(Generic::new(fd, Interest::READ, CMode::Level), |_, _, app: &mut super::App| {
            evenements(app);
            Ok(PostAction::Continue)
        })
        .map_err(|e| e.to_string())?;
    while !app.exit {
        boucle.dispatch(Duration::from_millis(500), &mut app).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Événements X11 → la même logique que sous Wayland (pointeur, clavier).
fn evenements(app: &mut super::App) {
    use smithay_client_toolkit::seat::pointer::{AxisScroll, PointerEventKind as K};
    let mut lus = vec![];
    if let Some(x) = app.x11.as_ref() {
        while let Ok(Some(ev)) = x.c.poll_for_event() {
            lus.push(ev);
        }
    }
    let mut redessiner = false;
    for ev in lus {
        match ev {
            Event::ButtonPress(e) | Event::ButtonRelease(e) => {
                let appui = e.response_type & 0x7f == xproto::BUTTON_PRESS_EVENT;
                let pos = (e.event_x as f32, e.event_y as f32);
                let kind = match e.detail {
                    4 | 5 if appui => {
                        let d = if e.detail == 4 { -1 } else { 1 };
                        let v = AxisScroll { absolute: d as f64 * 10.0, discrete: d, ..Default::default() };
                        K::Axis { time: e.time, horizontal: AxisScroll::default(), vertical: v, source: None }
                    }
                    4 | 5 => continue,
                    b => {
                        let button = match b {
                            1 => super::BTN_LEFT,
                            2 => super::BTN_MIDDLE,
                            3 => super::BTN_RIGHT,
                            _ => continue,
                        };
                        if appui { K::Press { time: e.time, button, serial: 0 } } else { K::Release { time: e.time, button, serial: 0 } }
                    }
                };
                redessiner |= app.pointeur(&kind, pos);
            }
            Event::MotionNotify(e) => redessiner |= app.pointeur(&K::Motion { time: e.time }, (e.event_x as f32, e.event_y as f32)),
            Event::EnterNotify(e) => redessiner |= app.pointeur(&K::Enter { serial: 0 }, (e.event_x as f32, e.event_y as f32)),
            Event::LeaveNotify(e) => redessiner |= app.pointeur(&K::Leave { serial: 0 }, (e.event_x as f32, e.event_y as f32)),
            Event::KeyPress(e) => {
                let etat = u16::from(e.state);
                let maj = etat & (u16::from(xproto::KeyButMask::SHIFT) | u16::from(xproto::KeyButMask::LOCK)) != 0;
                app.ctrl = etat & u16::from(xproto::KeyButMask::CONTROL) != 0;
                app.shift = etat & u16::from(xproto::KeyButMask::SHIFT) != 0;
                let sym = app.x11.as_ref().map(|x| x.symbole(e.detail, maj)).unwrap_or(0);
                let keysym = smithay_client_toolkit::seat::keyboard::Keysym::from(sym);
                let utf8 = keysym.key_char().filter(|c| !c.is_control() || *c == '\r').map(|c| c.to_string());
                app.on_key(smithay_client_toolkit::seat::keyboard::KeyEvent { time: e.time, raw_code: e.detail as u32, keysym, utf8 });
                redessiner = true;
            }
            Event::FocusIn(_) => app.kb_focus = true,
            Event::FocusOut(_) => app.kb_focus = false,
            Event::Expose(_) => redessiner = true,
            _ => {}
        }
    }
    if redessiner {
        app.render();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn swaps_channels() {
        assert_eq!(super::vers_bgra(&[1, 2, 3, 4, 5, 6, 7, 8]), vec![3, 2, 1, 4, 7, 6, 5, 8]);
    }
}
