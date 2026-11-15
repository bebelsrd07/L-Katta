//! Essai du glisser-déposer X11 : joue le rôle d'un gestionnaire de fichiers qui glisse des
//! fichiers sur le maki (fenêtre de classe « lkatta ») par le protocole XDND.
//!
//!   cargo run --example xdnd_source -- /chemin/fichier1 /chemin/fichier2
//!
//! Affiche la réponse du maki (accepté ou non, fin du dépôt).

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::wrapper::ConnectionExt as _;

fn atome(c: &impl Connection, n: &str) -> u32 {
    c.intern_atom(false, n.as_bytes()).unwrap().reply().unwrap().atom
}

fn main() {
    let fichiers: Vec<String> = std::env::args().skip(1).collect();
    let (c, n) = x11rb::connect(None).expect("serveur X");
    let racine = c.setup().roots[n].root;
    // La fenêtre du maki : un enfant de la racine de classe « lkatta », qui accepte XDND.
    let aware = atome(&c, "XdndAware");
    let cible = c
        .query_tree(racine)
        .unwrap()
        .reply()
        .unwrap()
        .children
        .into_iter()
        .find(|w| {
            let cl = c.get_property(false, *w, xproto::AtomEnum::WM_CLASS, xproto::AtomEnum::ANY, 0, 64).unwrap().reply().unwrap();
            let a = c.get_property(false, *w, aware, xproto::AtomEnum::ANY, 0, 1).unwrap().reply().unwrap();
            cl.value.starts_with(b"lkatta") && !a.value.is_empty()
        })
        .expect("pas de fenêtre lkatta XdndAware");
    let moi = c.generate_id().unwrap();
    c.create_window(0, moi, racine, 0, 0, 1, 1, 0, xproto::WindowClass::INPUT_OUTPUT, 0, &xproto::CreateWindowAux::new()).unwrap();
    let [sel, enter, pos, status, drop, finished, copie, uri] =
        ["XdndSelection", "XdndEnter", "XdndPosition", "XdndStatus", "XdndDrop", "XdndFinished", "XdndActionCopy", "text/uri-list"].map(|a| atome(&c, a));
    c.set_selection_owner(moi, sel, x11rb::CURRENT_TIME).unwrap();
    let envoyer = |t: u32, d: [u32; 5]| {
        c.send_event(false, cible, xproto::EventMask::NO_EVENT, xproto::ClientMessageEvent::new(32, cible, t, d)).unwrap();
        c.flush().unwrap();
    };
    let g = c.get_geometry(cible).unwrap().reply().unwrap();
    let (x, y) = (g.x as u32 + g.width as u32 / 2, g.y as u32 + g.height as u32 / 2);
    envoyer(enter, [moi, 5 << 24, uri, 0, 0]);
    envoyer(pos, [moi, 0, (x << 16) | y, 0, copie]);
    let corps = fichiers.iter().map(|f| format!("file://{}\r\n", f.replace(' ', "%20"))).collect::<String>();
    let debut = std::time::Instant::now();
    let mut lache = false;
    while debut.elapsed().as_secs() < 8 {
        let Some(ev) = c.poll_for_event().unwrap() else {
            std::thread::sleep(std::time::Duration::from_millis(20));
            continue;
        };
        match ev {
            Event::ClientMessage(e) if e.type_ == status => {
                let d = e.data.as_data32();
                println!("statut : accepté={} action copie={}", d[1] & 1 == 1, d[4] == copie);
                if !lache {
                    lache = true;
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    envoyer(drop, [moi, 0, x11rb::CURRENT_TIME, 0, 0]);
                }
            }
            Event::SelectionRequest(r) => {
                c.change_property8(xproto::PropMode::REPLACE, r.requestor, r.property, r.target, corps.as_bytes()).unwrap();
                let rep = xproto::SelectionNotifyEvent { response_type: xproto::SELECTION_NOTIFY_EVENT, sequence: 0, time: r.time, requestor: r.requestor, selection: r.selection, target: r.target, property: r.property };
                c.send_event(false, r.requestor, xproto::EventMask::NO_EVENT, rep).unwrap();
                c.flush().unwrap();
                println!("liste envoyée : {} fichier(s)", fichiers.len());
            }
            Event::ClientMessage(e) if e.type_ == finished => {
                let d = e.data.as_data32();
                println!("fini : accepté={} action copie={}", d[1] & 1 == 1, d[2] == copie);
                return;
            }
            _ => {}
        }
    }
    println!("pas de réponse");
}
