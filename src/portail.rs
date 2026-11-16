//! La fenêtre « Ouvrir » du bureau, sous Linux.
//!
//! Le portail de fichiers (`org.freedesktop.portal.FileChooser`) est la fenêtre habituelle du
//! système — celle de KDE, GNOME, wlroots. C'est elle qu'on ouvre plutôt qu'une fenêtre à nous :
//! l'utilisateur y retrouve ses raccourcis, ses derniers dossiers, et sous Flatpak c'est la seule
//! qui ait le droit de voir hors du bac à sable.
//!
//! Par D-Bus, donc, ce qui n'existe que sous Linux : `zbus` n'est plus une dépendance que pour
//! cette cible (voir `Cargo.toml`). Ailleurs, `portail_ailleurs.rs` expose la même fonction et
//! dit ce qu'il ne sait pas faire ; `main.rs` choisit entre les deux par une `cfg`.
//!
//! Ce module ne contient que l'appel au portail. Le repli (zenity, yad, kdialog) et tout ce qui
//! lit les fichiers choisis restent dans `fichiers.rs`, qui eux sont portables.

use std::collections::HashMap;
use std::path::PathBuf;

use zbus::{OwnedObjectPath, OwnedVariant as OwnedValue, Variant as Value};

pub fn ouvrir(titre: &str) -> Result<Vec<PathBuf>, String> {
    use futures_lite::StreamExt;
    let titre = titre.to_string();
    zbus::block_on(async move {
        let c = zbus::Connection::session().await.map_err(|e| e.to_string())?;
        // Le portail répond par un signal sur un chemin calculé d'avance : on l'écoute avant
        // d'appeler, pour ne pas manquer une réponse rapide.
        let jeton = format!("lkatta{}", fastrand::u32(..));
        let moi = c.unique_name().map(|n| n.trim_start_matches(':').replace('.', "_")).unwrap_or_default();
        let chemin = format!("/org/freedesktop/portal/desktop/request/{moi}/{jeton}");
        let regle = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.portal.Request")
            .member("Response")
            .path(chemin.as_str())
            .build();
        let regle = regle.map_err(|e| e.to_string())?;
        let mut flux = zbus::MessageStream::for_match_rule(regle, &c, std::num::NonZeroUsize::new(4)).await.map_err(|e| e.to_string())?;
        let mut options: HashMap<&str, Value> = HashMap::new();
        options.insert("handle_token", Value::from(jeton.as_str()));
        options.insert("multiple", Value::from(true));
        options.insert("modal", Value::from(true));
        let r = c
            .call_method(Some("org.freedesktop.portal.Desktop"), "/org/freedesktop/portal/desktop", Some("org.freedesktop.portal.FileChooser"), "OpenFile", &("", titre.as_str(), options))
            .await
            .map_err(|e| e.to_string())?;
        let _: OwnedObjectPath = r.body().deserialize().map_err(|e| e.to_string())?;
        // L'utilisateur prend le temps qu'il veut ; la fenêtre est celle du bureau.
        while let Some(m) = flux.next().await {
            let Ok(m) = m else { continue };
            let (code, res): (u32, HashMap<String, OwnedValue>) = m.body().deserialize().map_err(|e| e.to_string())?;
            if code != 0 {
                return Ok(vec![]);
            }
            let uris: Vec<String> = res.get("uris").and_then(|v| Vec::<String>::try_from(v.clone()).ok()).unwrap_or_default();
            return Ok(uris.iter().filter_map(|u| crate::fichiers::depuis_uri(u)).collect());
        }
        Err("portail muet".to_string())
    })
}
