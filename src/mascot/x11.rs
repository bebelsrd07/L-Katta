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

#[cfg(test)]
mod tests {
    #[test]
    fn swaps_channels() {
        assert_eq!(super::vers_bgra(&[1, 2, 3, 4, 5, 6, 7, 8]), vec![3, 2, 1, 4, 7, 6, 5, 8]);
    }
}
