//! GNOME Shell : petite extension qui sert de pont (assets/gnome/), comme le script KWin sous
//! KDE. GNOME ne laisse pas une application voir les autres fenêtres ; l'extension envoie la
//! fenêtre active au service `org.lkatta.Maki` et offre, chez `org.gnome.Shell`, l'objet
//! `/org/lkatta/Gnome` (liste des fenêtres, actions).
//!
//! Installée seulement quand l'utilisateur le demande (`/gnome`, `lkatta gnome`).

use std::process::Stdio;

pub const UUID: &str = "lkatta@raantss18.github.io";
const METADATA: &str = include_str!("../assets/gnome/lkatta@raantss18.github.io/metadata.json");
const EXTENSION: &str = include_str!("../assets/gnome/lkatta@raantss18.github.io/extension.js");

const SHELL: &str = "org.gnome.Shell";
const CHEMIN: &str = "/org/lkatta/Gnome";
const INTERFACE: &str = "org.lkatta.Gnome";

/// Le bureau est-il GNOME (ou dérivé : Ubuntu, Pop!_OS…) ?
pub fn bureau_gnome() -> bool {
    crate::environnement::courant().compositeur == "gnome"
}

fn dossier() -> std::path::PathBuf {
    // Dans le Flatpak, XDG_DATA_HOME est celui du bac à sable : GNOME Shell lit celui de la machine.
    std::env::var_os(if crate::hote::flatpak() { "HOST_XDG_DATA_HOME" } else { "XDG_DATA_HOME" })
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| crate::paths::home_dir().join(".local/share"))
        .join("gnome-shell/extensions")
        .join(UUID)
}

/// Appel D-Bus à l'objet de l'extension, chez GNOME Shell.
fn appeler<A, R>(interface: &str, methode: &str, args: &A) -> Result<R, String>
where
    A: serde::Serialize + zbus::zvariant::DynamicType,
    R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
{
    zbus::block_on(async {
        let c = zbus::Connection::session().await.map_err(|e| e.to_string())?;
        let r = c.call_method(Some(SHELL), CHEMIN, Some(interface), methode, args).await.map_err(|_| absente())?;
        r.body().deserialize::<R>().map_err(|e| e.to_string())
    })
}

/// L'extension tourne-t-elle dans GNOME Shell (objet présent sur le bus) ?
pub fn active() -> bool {
    appeler::<_, zbus::OwnedVariant>("org.freedesktop.DBus.Properties", "Get", &(INTERFACE, "Version")).is_ok()
}

/// Fenêtres vues par GNOME Shell.
pub fn fenetres() -> Result<Vec<crate::toplevels::Fenetre>, String> {
    let json: String = appeler(INTERFACE, "Fenetres", &())?;
    Ok(depuis_json(&json))
}

/// Liste JSON envoyée par l'extension → fenêtres.
pub fn depuis_json(json: &str) -> Vec<crate::toplevels::Fenetre> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    v.as_array()
        .into_iter()
        .flatten()
        .map(|f| {
            let b = |k: &str| f[k].as_bool().unwrap_or(false);
            let s = |k: &str| f[k].as_str().unwrap_or("").to_string();
            crate::toplevels::Fenetre {
                id: f["id"].as_u64().unwrap_or(0) as u32,
                titre: s("titre"),
                classe: s("classe"),
                active: b("active"),
                plein_ecran: b("plein"),
                agrandie: b("agrandie"),
                reduite: b("reduite"),
            }
        })
        .collect()
}

/// Action sur une fenêtre : activer, reduire, agrandir, fermer.
pub fn action(id: u32, a: &str) -> Result<(), String> {
    let ok: bool = appeler(INTERFACE, "Action", &(id, a))?;
    if ok { Ok(()) } else { Err(t!("GNOME ne trouve plus cette fenêtre (fermée entre-temps ?)").into()) }
}

fn absente() -> String {
    t!("l'extension GNOME de L Katta n'est pas active : tape /gnome pour l'installer").into()
}

fn lancer(prog: &str, args: &[&str]) -> bool {
    crate::config::which(prog) && crate::hote::commande(prog).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// `['a@b', 'c@d']` (gsettings) → liste.
pub fn liste_gsettings(t: &str) -> Vec<String> {
    let t = t.trim().trim_start_matches("@as").trim();
    t.trim_start_matches('[').trim_end_matches(']').split(',').map(|s| s.trim().trim_matches('\'').to_string()).filter(|s| !s.is_empty()).collect()
}

/// Ajoute l'extension à la liste des extensions activées (sans `gnome-extensions`).
fn activer_gsettings() -> bool {
    let Ok(o) = crate::hote::commande("gsettings").args(["get", "org.gnome.shell", "enabled-extensions"]).output() else { return false };
    let mut l = liste_gsettings(&String::from_utf8_lossy(&o.stdout));
    if !l.iter().any(|u| u == UUID) {
        l.push(UUID.to_string());
    }
    lancer("gsettings", &["set", "org.gnome.shell", "enabled-extensions", &format_liste(&l)])
}

/// Installe (ou met à jour) l'extension et l'active. Renvoie le message pour l'utilisateur.
pub fn installer() -> Result<String, String> {
    let d = dossier();
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    std::fs::write(d.join("metadata.json"), METADATA).map_err(|e| e.to_string())?;
    std::fs::write(d.join("extension.js"), EXTENSION).map_err(|e| e.to_string())?;
    let deja = active();
    // `gnome-extensions enable` ne connaît une extension nouvelle qu'après redémarrage de
    // GNOME Shell : la liste gsettings la fait charger à la prochaine session.
    let activee = lancer("gnome-extensions", &["enable", UUID]) || activer_gsettings();
    if !activee {
        return Err(t!("extension copiée dans {}, mais je n'ai pas pu l'activer : ouvre « Extensions » et active « L Katta »", d.display()).into());
    }
    Ok(if deja {
        t!("Extension GNOME à jour et active : je vois tes fenêtres.").into()
    } else {
        t!("Extension GNOME installée. Ferme ta session et rouvre-la une fois (GNOME ne charge une nouvelle extension qu'à l'ouverture de session) : ensuite je verrai tes fenêtres, je me cacherai devant le plein écran et je pourrai les manipuler à ta demande.").into()
    })
}

/// Au démarrage, sous GNOME : extension installée → fichiers remis à jour (pris en compte à la
/// prochaine session) ; absente → une proposition, une seule fois. Renvoie le mot à dire.
pub fn au_demarrage() -> Option<String> {
    if !bureau_gnome() {
        return None;
    }
    let d = dossier();
    if d.join("extension.js").exists() {
        let vieux = std::fs::read_to_string(d.join("extension.js")).unwrap_or_default() != EXTENSION
            || std::fs::read_to_string(d.join("metadata.json")).unwrap_or_default() != METADATA;
        if vieux {
            let _ = std::fs::write(d.join("metadata.json"), METADATA);
            let _ = std::fs::write(d.join("extension.js"), EXTENSION);
        }
        return None;
    }
    let marque = crate::paths::data_dir().join("gnome-propose");
    if marque.exists() || active() {
        return None;
    }
    let _ = std::fs::write(&marque, b"1");
    Some(t!("Sous GNOME, je ne vois pas tes fenêtres sans une petite extension (je me cacherais devant tes vidéos en plein écran, je pourrais les manipuler à ta demande). Tape /gnome dans la bulle pour l'installer.").into())
}

/// « Ne pas déranger » de GNOME : les bannières de notification sont coupées.
pub fn ne_pas_deranger() -> bool {
    gsettings_get("org.gnome.desktop.notifications", "show-banners").is_some_and(|v| v.trim() == "false")
}

fn gsettings_get(schema: &str, cle: &str) -> Option<String> {
    if !crate::config::which("gsettings") {
        return None;
    }
    let o = crate::hote::commande("gsettings").args(["get", schema, cle]).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

// --- raccourci global (raccourcis personnalisés de GNOME) --------------------------------

const TOUCHES: &str = "org.gnome.settings-daemon.plugins.media-keys";
const PERSO: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/lkatta/";

/// « Meta+K », « Ctrl+Alt+Espace », « Meta+F2 » → « <Super>k », « <Control><Alt>space »,
/// « <Super>F2 » (notation GTK de GNOME).
pub fn touches_gnome(seq: &str) -> Option<String> {
    let mut mods = String::new();
    let mut touche = None;
    for part in seq.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_lowercase().as_str() {
            "meta" | "super" | "win" => mods.push_str("<Super>"),
            "ctrl" | "control" => mods.push_str("<Control>"),
            "alt" => mods.push_str("<Alt>"),
            "shift" | "maj" => mods.push_str("<Shift>"),
            "space" | "espace" => touche = Some("space".to_string()),
            p if p.len() == 1 && p.chars().all(|c| c.is_ascii_alphanumeric()) => touche = Some(p.to_string()),
            p if p.starts_with('f') && p[1..].parse::<u32>().is_ok_and(|n| (1..=24).contains(&n)) => touche = Some(p.to_uppercase()),
            _ => return None,
        }
    }
    touche.map(|t| mods + &t)
}

fn perso_get(cle: &str, chemin: &str) -> Option<String> {
    gsettings_get(&format!("{TOUCHES}.custom-keybinding:{chemin}"), cle).map(|v| v.trim_matches('\'').to_string())
}

fn perso_set(cle: &str, valeur: &str) -> bool {
    lancer("gsettings", &["set", &format!("{TOUCHES}.custom-keybinding:{PERSO}"), cle, valeur])
}

/// Inscrit le raccourci global dans GNOME (Paramètres › Clavier › Raccourcis personnalisés) :
/// il lance `lkatta raccourci`, qui ouvre la bulle. Vide ou « aucun » : retiré. Rien n'est
/// réécrit s'il est déjà en place, et une combinaison déjà prise par un autre raccourci
/// personnalisé n'est pas volée.
pub fn raccourci(seq: &str) {
    let seq = seq.trim();
    if seq.is_empty() || seq == "aucun" {
        retirer_raccourci();
        return;
    }
    let Some(liste) = gsettings_get(TOUCHES, "custom-keybindings") else { return };
    let Some(touches) = touches_gnome(seq) else {
        eprintln!("lkatta : raccourci « {seq} » incompris (ex. Meta+K)");
        return;
    };
    let mut l = liste_gsettings(&liste);
    let prise = l.iter().filter(|c| c.as_str() != PERSO).any(|c| perso_get("binding", c).is_some_and(|b| b.eq_ignore_ascii_case(&touches)));
    if prise {
        eprintln!("lkatta : « {touches} » sert déjà à un autre raccourci GNOME ; le mien n'est pas posé");
        return;
    }
    let commande = format!("{} raccourci", crate::hote::lanceur_bureau());
    if perso_get("binding", PERSO).as_deref() != Some(touches.as_str()) {
        perso_set("binding", &touches);
    }
    if perso_get("command", PERSO).as_deref() != Some(commande.as_str()) {
        perso_set("command", &commande);
    }
    if perso_get("name", PERSO).as_deref() != Some("L Katta") {
        perso_set("name", "L Katta");
    }
    if !l.iter().any(|c| c == PERSO) {
        l.push(PERSO.to_string());
        lancer("gsettings", &["set", TOUCHES, "custom-keybindings", &format_liste(&l)]);
    }
}

/// Retire le raccourci de L Katta des raccourcis personnalisés de GNOME.
pub fn retirer_raccourci() {
    let Some(liste) = gsettings_get(TOUCHES, "custom-keybindings") else { return };
    let l = liste_gsettings(&liste);
    if l.iter().any(|c| c == PERSO) {
        let reste: Vec<String> = l.into_iter().filter(|c| c != PERSO).collect();
        lancer("gsettings", &["set", TOUCHES, "custom-keybindings", &format_liste(&reste)]);
        for cle in ["name", "command", "binding"] {
            lancer("gsettings", &["reset", &format!("{TOUCHES}.custom-keybinding:{PERSO}"), cle]);
        }
    }
}

fn format_liste(l: &[String]) -> String {
    format!("[{}]", l.iter().map(|u| format!("'{u}'")).collect::<Vec<_>>().join(", "))
}

/// Retire l'extension (désactivée et effacée).
pub fn retirer() -> Result<String, String> {
    let _ = lancer("gnome-extensions", &["disable", UUID]);
    if let Ok(o) = crate::hote::commande("gsettings").args(["get", "org.gnome.shell", "enabled-extensions"]).output() {
        let l: Vec<String> = liste_gsettings(&String::from_utf8_lossy(&o.stdout)).into_iter().filter(|u| u != UUID).collect();
        let _ = lancer("gsettings", &["set", "org.gnome.shell", "enabled-extensions", &format_liste(&l)]);
    }
    let d = dossier();
    if d.exists() {
        std::fs::remove_dir_all(&d).map_err(|e| e.to_string())?;
    }
    Ok(t!("Extension GNOME retirée.").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list_and_settings() {
        let l = depuis_json(r#"[{"id":12,"titre":"Firefox","classe":"firefox","active":true,"plein":true,"agrandie":false,"reduite":false},{"id":3,"titre":"Fichiers","classe":"org.gnome.Nautilus"}]"#);
        assert_eq!(l.len(), 2);
        assert!(l[0].active && l[0].plein_ecran && l[0].id == 12);
        assert_eq!(l[1].classe, "org.gnome.Nautilus");
        assert!(depuis_json("pas du json").is_empty());
        assert_eq!(liste_gsettings("['a@b', 'c@d']"), vec!["a@b", "c@d"]);
        assert!(liste_gsettings("@as []").is_empty());
        assert_eq!(touches_gnome("Meta+K").as_deref(), Some("<Super>k"));
        assert_eq!(touches_gnome("ctrl+alt+espace").as_deref(), Some("<Control><Alt>space"));
        assert_eq!(touches_gnome("Meta+F2").as_deref(), Some("<Super>F2"));
        assert_eq!(touches_gnome("Meta+Truc"), None);
        assert_eq!(format_liste(&["/a/".into(), "/b/".into()]), "['/a/', '/b/']");
        // L'extension embarquée est cohérente avec son identifiant.
        assert!(METADATA.contains(UUID) && EXTENSION.contains("org.lkatta.Gnome"));
    }
}
