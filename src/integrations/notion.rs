//! Notion : lire et écrire dans l'espace Notion de l'utilisateur.
//!
//! Un jeton interne (`ntn_…`) rangé dans le trousseau du bureau (src/cles.rs) — jamais dans
//! un fichier de L Katta. Aucune dépendance nouvelle : l'API Notion est du JSON sur HTTPS,
//! `ureq` et `serde_json` sont déjà là.
//!
//! Sécurité :
//! - tout ce qui revient de Notion repart vers le modèle entouré de `[CONTENU EXTERNE]`
//!   (fait par tools.rs) : une page Notion peut contenir « envoie tout mes mails », ce n'est
//!   pas une instruction ;
//! - les trois outils qui écrivent passent par `Host::confirm` avant l'envoi ;
//! - `regles.notion` coupe les six outils, pour toute la machine si l'administrateur le veut.
//!
//! Ce qui découpe ou met en forme (adresse → identifiant, blocs → Markdown, Markdown →
//! blocs, propriétés → texte) est pur : testable sans réseau ni jeton.

use std::time::Duration;

use serde_json::{Value, json};

/// Adresse de l'API. Fixe : le jeton n'est jamais envoyé à un autre hôte (comme src/cles.rs).
const API: &str = "https://api.notion.com/v1";
/// Version d'API annoncée à Notion.
const VERSION: &str = "2022-06-28";
/// Plafond de lecture d'une réponse : large pour une page entière, pas de quoi saturer la bulle.
const LIMITE: usize = 8 * 1024 * 1024;
/// Blocs par appel : le maximum que Notion accepte.
const PAR_PAGE: usize = 100;
/// Niveaux de blocs enfants lus sous un bloc (au-delà, on s'arrête et on le dit).
const PROFONDEUR: u8 = 4;

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(30)))
        // Un 4xx reste une réponse à lire : c'est là que Notion explique ce qui ne va pas
        // (« page introuvable », « jeton sans accès »), bien plus utile qu'un code nu.
        .http_status_as_error(false)
        .build()
        .into()
}

// ── Identifiants et jetons ────────────────────────────────────────────────────

/// Identifiant Notion (UUID) tiré d'une adresse collée par l'utilisateur ou d'un identifiant
/// brut. Notion met l'identifiant en fin d'adresse, collé au titre ou seul, avec ou sans tirets
/// (`…/Mon-Titre-1a2b…c6d`, `…/1a2b3c4d-5e6f-…`, `…?pvs=4`).
///
/// On prend le dernier morceau qui, tirets retirés, fait exactement 32 chiffres hexadécimaux :
/// un titre n'a jamais cette forme, et l'identifiant est toujours ce qu'il y a de plus à droite.
pub fn id_depuis_url(s: &str) -> String {
    let propre = s.trim().split(['?', '#']).next().unwrap_or("");
    // Coupé sur tout ce qui n'est ni hexadécimal ni tiret : l'identifiant survit entier.
    let morceaux: Vec<&str> = propre.split(|c: char| !(c.is_ascii_hexdigit() || c == '-')).filter(|m| !m.is_empty()).collect();
    for m in morceaux.iter().rev() {
        let plat: String = m.chars().filter(|c| *c != '-').map(|c| c.to_ascii_lowercase()).collect();
        if plat.len() == 32 && plat.chars().all(|c| c.is_ascii_hexdigit()) {
            return uuid(&plat);
        }
    }
    String::new()
}

/// 32 hexadécimaux → UUID à tirets, la forme que Notion renvoie lui-même dans ses `id`.
fn uuid(h: &str) -> String {
    if h.len() != 32 {
        return h.to_string();
    }
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// Un jeton Notion plausible (pas un texte à remplacer ni un mot de passe recopié ailleurs).
/// Les jetons d'intégration commencent par `ntn_` ; les anciens secrets par `secret_`.
pub fn jeton_plausible(k: &str) -> bool {
    let k = k.trim();
    k.len() >= 40 && !k.contains(char::is_whitespace) && (k.starts_with("ntn_") || k.starts_with("secret_"))
}

// ── Client ───────────────────────────────────────────────────────────────────

/// Un client Notion : le jeton du trousseau et de quoi parler à l'API.
pub struct Client {
    jeton: String,
    agent: ureq::Agent,
}

impl Client {
    /// Le client, si un jeton est rangé dans le trousseau. Sinon une erreur qui dit quoi faire.
    pub fn nouveau() -> Result<Client, String> {
        match crate::cles::notion() {
            Some(jeton) => Ok(Client { jeton, agent: agent() }),
            None => Err(t!("aucun jeton Notion : onglet Intégrations de la bulle, ou « lkatta notion <jeton> » dans un terminal").to_string()),
        }
    }

    /// Un appel à l'API. `methode` : GET, POST ou PATCH ; `corps` : JSON envoyé (POST/PATCH).
    fn appel(&self, methode: &str, chemin: &str, corps: Option<Value>) -> Result<Value, String> {
        let url = format!("{API}{chemin}");
        let auth = format!("Bearer {}", self.jeton);
        // `Agent::get` renvoie un constructeur SANS corps, `post` et `patch` un constructeur
        // AVEC corps : deux types distincts, donc impossible de préparer la requête une fois
        // pour toutes puis de choisir la méthode. Chaque branche construit sa réponse, et les
        // trois aboutissent au même `Result<Response, Error>`.
        //
        // Un POST ou un PATCH sans corps retombe sur un GET : aucun appel de ce module ne fait
        // ça, et mieux vaut une lecture refusée par Notion qu'un envoi à moitié construit.
        let mut r = match (methode, corps) {
            ("POST", Some(c)) => self.agent.post(&url).header("Authorization", &auth).header("Notion-Version", VERSION).header("Content-Type", "application/json").send_json(&c),
            ("PATCH", Some(c)) => self.agent.patch(&url).header("Authorization", &auth).header("Notion-Version", VERSION).header("Content-Type", "application/json").send_json(&c),
            _ => self.agent.get(&url).header("Authorization", &auth).header("Notion-Version", VERSION).header("Content-Type", "application/json").call(),
        }
        // Le jeton n'apparaît jamais dans un message d'erreur : il resterait dans les journaux.
        .map_err(|e| erreur_reseau(&e))?;
        let statut = r.status().as_u16();
        let v: Value = r.body_mut().with_config().limit(LIMITE).read_json().unwrap_or_default();
        if statut >= 400 {
            return Err(erreur_api(statut, &v));
        }
        Ok(v)
    }

    /// Ce que le jeton peut voir : le bot lui-même et son espace. Sert de test de connexion.
    pub fn moi(&self) -> Result<String, String> {
        let v = self.appel("GET", "/users/me", None)?;
        let nom = v.get("name").and_then(Value::as_str).unwrap_or("");
        let courriel = v.pointer("/person/email").and_then(Value::as_str).unwrap_or("");
        Ok(match (nom, courriel) {
            (n, c) if !n.is_empty() && !c.is_empty() => format!("{n} · {c}"),
            (n, _) if !n.is_empty() => n.to_string(),
            _ => t!("jeton accepté").to_string(),
        })
    }

    /// Cherche dans tout ce que le jeton peut voir ; renvoie des lignes lisibles.
    pub fn chercher(&self, requete: &str, nombre: usize) -> Result<String, String> {
        let n = nombre.clamp(1, PAR_PAGE);
        let corps = json!({"query": requete, "page_size": n});
        let r = self.appel("POST", "/search", Some(corps))?;
        let vus = r.get("results").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
        if vus == 0 {
            return Ok(t!("rien trouvé dans Notion pour « {} »", requete));
        }
        let mut out = format!("{}\n", t!("{} résultat(s) Notion pour « {} »", vus, requete));
        for o in r.get("results").and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or(&[]) {
            out.push('\n');
            out.push_str(&resume_objet(o));
        }
        Ok(out)
    }

    /// Une page en Markdown : titre, adresse, puis ses blocs (et leurs enfants).
    pub fn lire_page(&self, cible: &str) -> Result<String, String> {
        let id = id_depuis_url(cible);
        if id.is_empty() {
            return Err(t!("je ne trouve pas d'identifiant Notion dans « {} »", cible));
        }
        let page = self.appel("GET", &format!("/pages/{id}"), None)?;
        let titre = titre_page(&page);
        let mut out = format!("# {}\n", if titre.is_empty() { t!("(sans titre)").to_string() } else { titre });
        if let Some(u) = page.get("url").and_then(Value::as_str) {
            out.push_str(&format!("{u}\n"));
        }
        if let Some(t) = page.get("last_edited_time").and_then(Value::as_str) {
            out.push_str(&format!("{}\n", t!("modifiée le {}", heure_courte(t))));
        }
        out.push('\n');
        let props = proprietes_page(&page);
        if !props.is_empty() {
            out.push_str(&props);
            out.push('\n');
        }
        out.push_str(&self.enfants(&id, PROFONDEUR)?);
        Ok(out)
    }

    /// Les lignes d'une base, avec leurs propriétés. Un filtre simple est bâti d'après le VRAI
    /// type de la propriété (lu dans la base), pour ne pas deviner et échouer.
    pub fn requete_base(&self, cible: &str, propriete: &str, valeur: &str, nombre: usize) -> Result<String, String> {
        let id = id_depuis_url(cible);
        if id.is_empty() {
            return Err(t!("je ne trouve pas d'identifiant de base Notion dans « {} »", cible));
        }
        let base = self.appel("GET", &format!("/databases/{id}"), None)?;
        let n = nombre.clamp(1, PAR_PAGE);
        let mut corps = json!({"page_size": n});
        if !propriete.trim().is_empty() {
            let nom = nom_propriete(&base, propriete);
            let tipe = base.pointer("/properties").and_then(|p| p.get(&nom)).and_then(|p| p.get("type")).and_then(Value::as_str).unwrap_or("");
            let filtre = match filtre_propriete(&nom, tipe, valeur) {
                Some(f) => f,
                None => return Err(t!("je ne sais pas filtrer la propriété « {} » (type {})", nom, if tipe.is_empty() { t!("inconnu").to_string() } else { tipe.to_string() })),
            };
            corps["filter"] = filtre;
        }
        let r = self.appel("POST", &format!("/databases/{id}/query"), Some(corps))?;
        let rows = r.get("results").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
        let titre = texte_riche(base.get("title").unwrap_or(&Value::Null));
        if rows == 0 {
            return Ok(t!("{} : aucune ligne", if titre.is_empty() { t!("la base").to_string() } else { titre.to_string() }));
        }
        let mut out = format!("{}\n", t!("{} ligne(s) de {}", rows, if titre.is_empty() { t!("la base").to_string() } else { titre.to_string() }));
        for o in r.get("results").and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or(&[]) {
            out.push('\n');
            out.push_str(&resume_objet(o));
        }
        if r.get("has_more").and_then(Value::as_bool).unwrap_or(false) {
            out.push_str(&format!("\n{}", t!("(encore des lignes : augmente « nombre »)")));
        }
        Ok(out)
    }

    /// Crée une page sous un parent (base ou page), avec un titre et du Markdown en blocs.
    pub fn creer_page(&self, parente: &str, titre: &str, contenu: &str) -> Result<String, String> {
        let id = id_depuis_url(parente);
        if id.is_empty() {
            return Err(t!("je ne trouve pas d'identifiant Notion pour le parent « {} »", parente));
        }
        let blocs = blocs_depuis_markdown(contenu);
        let total = blocs.len();
        // Notion limite `children` à 100 blocs à la création : le reste suit en PATCH.
        let tete: Vec<Value> = blocs.into_iter().take(PAR_PAGE).collect();
        let deja = tete.len();
        // Base ou page ? Notion n'accepte pas les mêmes propriétés pour les deux, et le nom de
        // la propriété « titre » d'une base est libre : on le lit plutôt que de le deviner.
        let (parent, props) = match self.appel("GET", &format!("/databases/{id}"), None) {
            Ok(base) => {
                let nom_titre = base.pointer("/properties").and_then(Value::as_object).and_then(|ps| ps.iter().find(|(_, p)| p.get("type").and_then(Value::as_str) == Some("title")).map(|(n, _)| n.clone())).unwrap_or_else(|| "Name".to_string());
                let mut m = serde_json::Map::new();
                m.insert(nom_titre, json!({"title": [{"text": {"content": titre}}]}));
                (json!({"database_id": id}), Value::Object(m))
            }
            Err(_) => (json!({"page_id": id}), json!({"title": {"title": [{"text": {"content": titre}}]}})),
        };
        let cree = self.appel("POST", "/pages", Some(json!({"parent": parent, "properties": props, "children": tete})))?;
        let nouvelle = cree.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        if !nouvelle.is_empty() && total > deja {
            // Un échec ici ne défait pas la page déjà créée : on continue et on le dit.
            let suite = self.ajouter_blocs(&nouvelle, contenu, deja);
            if let Err(e) = suite {
                return Ok(format!("{}\n{}", t!("page créée : {}", titre_page(&cree)), t!("mais la suite du texte n'a pas pu être ajoutée : {}", e)));
            }
        }
        let t = titre_page(&cree);
        let t = if t.is_empty() { titre.to_string() } else { t };
        Ok(match cree.get("url").and_then(Value::as_str) {
            Some(u) if !u.is_empty() => format!("{}\n{u}", t!("page créée : {}", t)),
            _ => t!("page créée : {}", t),
        })
    }

    /// Ajoute du Markdown à la fin d'une page (ou d'un bloc qui a des enfants).
    pub fn ajouter_blocs(&self, cible: &str, contenu: &str, sauter: usize) -> Result<String, String> {
        let id = id_depuis_url(cible);
        if id.is_empty() {
            return Err(t!("je ne trouve pas d'identifiant Notion dans « {} »", cible));
        }
        let blocs: Vec<Value> = blocs_depuis_markdown(contenu).into_iter().skip(sauter).collect();
        if blocs.is_empty() {
            return Ok(t!("rien à ajouter").to_string());
        }
        let mut ajoute = 0usize;
        for lot in blocs.chunks(PAR_PAGE) {
            self.appel("PATCH", &format!("/blocks/{id}/children"), Some(json!({"children": lot})))?;
            ajoute += lot.len();
        }
        Ok(t!("{} bloc(s) ajouté(s)", ajoute))
    }

    /// Change les propriétés d'une page (statut, case à cocher, date, sélection, texte…).
    /// Le format Notion dépend du type de la propriété : on le lit dans la base parente.
    pub fn modifier_proprietes(&self, cible: &str, changements: &[(String, String)]) -> Result<String, String> {
        let id = id_depuis_url(cible);
        if id.is_empty() {
            return Err(t!("je ne trouve pas d'identifiant Notion dans « {} »", cible));
        }
        if changements.is_empty() {
            return Err(t!("aucun changement demandé").to_string());
        }
        let page = self.appel("GET", &format!("/pages/{id}"), None)?;
        // Les types viennent de la base parente, ou des propriétés déjà sur la page.
        let base_id = page.pointer("/parent/database_id").and_then(Value::as_str).unwrap_or("");
        let base = if base_id.is_empty() { Value::Null } else { self.appel("GET", &format!("/databases/{base_id}"), None).unwrap_or_default() };
        let mut props = serde_json::Map::new();
        for (nom, valeur) in changements {
            // La base parente connaît les vrais noms ; sinon la page elle-même.
            let depuis_base = nom_propriete(&base, nom);
            let cle = if depuis_base != nom.trim() { depuis_base } else { nom_propriete(&page, nom) };
            let tipe = base.pointer("/properties").and_then(|p| p.get(&cle)).and_then(|p| p.get("type")).and_then(Value::as_str).or_else(|| page.pointer("/properties").and_then(|p| p.get(&cle)).and_then(|p| p.get("type")).and_then(Value::as_str)).unwrap_or("");
            match propriete_notion(tipe, valeur) {
                Some(v) => {
                    props.insert(cle, v);
                }
                None => return Err(t!("je ne sais pas écrire la propriété « {} » (type {})", cle, if tipe.is_empty() { t!("inconnu").to_string() } else { tipe.to_string() })),
            }
        }
        let combien = props.len();
        self.appel("PATCH", &format!("/pages/{id}"), Some(json!({"properties": props})))?;
        Ok(t!("{} propriété(s) mise(s) à jour", combien))
    }

    /// Les blocs enfants de `id`, en Markdown, jusqu'à `profondeur` niveaux.
    fn enfants(&self, id: &str, profondeur: u8) -> Result<String, String> {
        if profondeur == 0 {
            return Ok("[…]\n".to_string());
        }
        let mut out = String::new();
        let mut curseur: Option<String> = None;
        // Une page longue vient en plusieurs appels ; on suit `next_cursor`.
        for _ in 0..40 {
            let mut chemin = format!("/blocks/{id}/children?page_size={PAR_PAGE}");
            if let Some(c) = &curseur {
                chemin.push_str(&format!("&start_cursor={c}"));
            }
            let r = self.appel("GET", &chemin, None)?;
            let blocs = r.get("results").and_then(Value::as_array).cloned().unwrap_or_default();
            for b in &blocs {
                let tipe = b.get("type").and_then(Value::as_str).unwrap_or("");
                let bid = b.get("id").and_then(Value::as_str).unwrap_or("");
                let mut ligne = bloc_markdown(b);
                // Un tableau se lit par ses lignes, qui sont ses enfants.
                let a_enfants = b.get("has_children").and_then(Value::as_bool).unwrap_or(false);
                if tipe == "table" && a_enfants && !bid.is_empty() {
                    ligne = self.tableau(bid, profondeur - 1).unwrap_or_default();
                } else if a_enfants && !bid.is_empty() && matches!(tipe, "toggle" | "column_list" | "column" | "bulleted_list_item" | "numbered_list_item" | "to_do" | "paragraph" | "quote" | "callout" | "heading_1" | "heading_2" | "heading_3") {
                    let sous = self.enfants(bid, profondeur - 1)?;
                    if !sous.trim().is_empty() {
                        ligne.push('\n');
                        // Deux espaces par niveau : le Markdown garde l'imbrication.
                        ligne.push_str(&indenter(&sous, if matches!(tipe, "bulleted_list_item" | "numbered_list_item" | "to_do") { "  " } else { "" }));
                    }
                }
                if !ligne.trim().is_empty() {
                    out.push_str(&ligne);
                    out.push('\n');
                }
            }
            match (r.get("has_more").and_then(Value::as_bool).unwrap_or(false), r.get("next_cursor").and_then(Value::as_str)) {
                (true, Some(c)) if !c.is_empty() => curseur = Some(c.to_string()),
                _ => break,
            }
        }
        Ok(out)
    }

    /// Un tableau Notion : ses lignes sont des blocs enfants `table_row`.
    fn tableau(&self, id: &str, profondeur: u8) -> Result<String, String> {
        let r = self.appel("GET", &format!("/blocks/{id}/children?page_size={PAR_PAGE}"), None)?;
        let rows = r.get("results").and_then(Value::as_array).cloned().unwrap_or_default();
        if rows.is_empty() || profondeur == 0 {
            return Ok(String::new());
        }
        let mut out = String::new();
        for (i, row) in rows.iter().enumerate() {
            let cellules = row.pointer("/table_row/cells").and_then(Value::as_array).cloned().unwrap_or_default();
            let textes: Vec<String> = cellules.iter().map(texte_riche).collect();
            out.push_str(&format!("| {} |\n", textes.join(" | ")));
            if i == 0 {
                out.push_str(&format!("|{}|\n", vec![" --- "; textes.len()].join("|")));
            }
        }
        Ok(out)
    }
}

// ── Erreurs ──────────────────────────────────────────────────────────────────

fn erreur_reseau(e: &ureq::Error) -> String {
    match e {
        ureq::Error::ConnectionFailed | ureq::Error::Io(_) => t!("Notion injoignable (connexion ou réseau)").to_string(),
        ureq::Error::HostNotFound => t!("adresse de Notion introuvable (DNS)").to_string(),
        other => t!("Notion : {}", raison(other)),
    }
}

fn erreur_api(statut: u16, v: &Value) -> String {
    let code = v.get("code").and_then(Value::as_str).unwrap_or("");
    let msg = v.get("message").and_then(Value::as_str).unwrap_or("");
    match statut {
        401 | 403 => t!("jeton Notion refusé ou sans accès à cette page : vérifie le jeton et les pages partagées avec l'intégration").to_string(),
        404 => t!("page ou base introuvable : elle n'est peut-être pas partagée avec l'intégration Notion").to_string(),
        429 => t!("Notion demande d'attendre (trop de requêtes) : réessaie dans quelques secondes").to_string(),
        _ if !msg.is_empty() => t!("Notion a refusé ({} : {}) {}", statut, code, msg),
        _ => t!("Notion a refusé (statut {})", statut),
    }
}

/// Le texte d'une erreur ureq, raccourci à ce qui tient dans une bulle.
///
/// Le jeton voyage dans un en-tête `Authorization`, jamais dans l'adresse ni dans un paramètre
/// d'URL : il ne peut donc pas apparaître ici. On garde la coupure courte pour ne pas noyer la
/// bulle sous une trace de pile réseau.
fn raison(e: &ureq::Error) -> String {
    e.to_string().chars().take(160).collect()
}

// ── Texte riche et blocs → Markdown ──────────────────────────────────────────

/// Entoure `s` de marques Markdown en laissant les espaces dehors : `** gras **` ne s'affiche
/// pas en gras, `**gras**` si.
fn habille(s: &str, avant: &str, apres: &str) -> String {
    let corps = s.trim();
    if corps.is_empty() || avant.is_empty() {
        return s.to_string();
    }
    let debut = &s[..s.len() - s.trim_start().len()];
    let fin = &s[s.trim_end().len()..];
    format!("{debut}{avant}{corps}{apres}{fin}")
}

/// Un tableau de texte riche Notion → une ligne Markdown (gras, italique, barré, code, lien).
pub fn texte_riche(v: &Value) -> String {
    let Some(liste) = v.as_array() else { return String::new() };
    let mut out = String::new();
    for t in liste {
        // Le texte est dans `/text/content`, une formule dans `/formula`, une équation dans
        // `/equation/expression` ; `plain_text` est le repli que Notion donne toujours.
        let brut = t.pointer("/text/content").and_then(Value::as_str).or_else(|| t.pointer("/equation/expression").and_then(Value::as_str)).or_else(|| t.get("plain_text").and_then(Value::as_str)).unwrap_or("");
        if brut.is_empty() {
            continue;
        }
        let a = t.get("annotations").unwrap_or(&Value::Null);
        let b = |k: &str| a.get(k).and_then(Value::as_bool).unwrap_or(false);
        let mut s = brut.to_string();
        if t.pointer("/equation/expression").and_then(Value::as_str).is_some() {
            s = format!("`${s}`");
        } else {
            if b("code") {
                s = habille(&s, "`", "`");
            }
            if b("bold") {
                s = habille(&s, "**", "**");
            }
            if b("italic") {
                s = habille(&s, "_", "_");
            }
            if b("strikethrough") {
                s = habille(&s, "~~", "~~");
            }
            if let Some(href) = t.get("href").and_then(Value::as_str).filter(|h| !h.is_empty()) {
                // Un format! et non un littéral : "]({href})" écrirait le mot « href », pas
                // l'adresse — le lien affiché pointerait nulle part.
                s = habille(&s, "[", &format!("]({href})"));
            }
        }
        out.push_str(&s);
    }
    out
}

/// Un bloc Notion → Markdown. Ce qu'on ne connaît pas est annoncé, jamais deviné : mieux vaut
/// un trou visible qu'un contenu inventé.
pub fn bloc_markdown(b: &Value) -> String {
    let tipe = b.get("type").and_then(Value::as_str).unwrap_or("");
    let corps = b.get(tipe).unwrap_or(&Value::Null);
    let rt = texte_riche(corps.get("rich_text").unwrap_or(&Value::Null));
    match tipe {
        "paragraph" => rt,
        "heading_1" => format!("# {rt}"),
        "heading_2" => format!("## {rt}"),
        "heading_3" => format!("### {rt}"),
        "bulleted_list_item" => format!("- {rt}"),
        "numbered_list_item" => format!("1. {rt}"),
        "to_do" => format!("- [{}] {rt}", if corps.get("checked").and_then(Value::as_bool).unwrap_or(false) { "x" } else { " " }),
        "toggle" => format!("▸ {rt}"),
        "quote" => format!("> {rt}"),
        "callout" => {
            let icone = corps.pointer("/icon/emoji").and_then(Value::as_str).unwrap_or("💡");
            format!("> {icone} {rt}")
        }
        "code" => {
            let lang = corps.get("language").and_then(Value::as_str).unwrap_or("");
            let l = if lang.is_empty() || lang == "plain text" { String::new() } else { lang.to_string() };
            format!("```{l}\n{rt}\n```")
        }
        "divider" => "---".into(),
        "equation" => format!("$$\n{}\n$$", corps.get("expression").and_then(Value::as_str).unwrap_or("")),
        "image" | "file" | "pdf" => {
            let url = corps.pointer("/external/url").and_then(Value::as_str).or_else(|| corps.pointer("/file/url").and_then(Value::as_str)).unwrap_or("");
            if url.is_empty() {
                rt
            } else {
                format!("[📎 {rt}]({url})")
            }
        }
        "bookmark" | "embed" | "link_preview" => {
            let url = corps.get("url").and_then(Value::as_str).unwrap_or("");
            if url.is_empty() {
                rt
            } else if rt.trim().is_empty() {
                url.to_string()
            } else {
                format!("[{rt}]({url})")
            }
        }
        "child_page" => format!("📄 {}", corps.get("title").and_then(Value::as_str).unwrap_or("")),
        "child_database" => format!("🗃 {}", corps.get("title").and_then(Value::as_str).unwrap_or("")),
        "table" | "column_list" | "column" => String::new(), // rendu par leurs enfants
        "table_row" => {
            let cellules = corps.get("cells").and_then(Value::as_array).cloned().unwrap_or_default();
            format!("| {} |", cellules.iter().map(texte_riche).collect::<Vec<_>>().join(" | "))
        }
        "breadcrumb" | "table_of_contents" => String::new(),
        "video" | "audio" => {
            let url = corps.pointer("/external/url").and_then(Value::as_str).or_else(|| corps.pointer("/file/url").and_then(Value::as_str)).unwrap_or("");
            if url.is_empty() {
                rt
            } else {
                format!("🎬 [{rt}]({url})")
            }
        }
        "unsupported" => format!("[{}]", t!("bloc non pris en charge par Notion")),
        _ => format!("[{}]", t!("bloc « {} » non rendu", tipe)),
    }
}

/// Indente chaque ligne non vide (l'imbrication Markdown des listes).
fn indenter(s: &str, prefixe: &str) -> String {
    if prefixe.is_empty() {
        return s.to_string();
    }
    s.lines().map(|l| if l.trim().is_empty() { l.to_string() } else { format!("{prefixe}{l}") }).collect::<Vec<_>>().join("\n")
}

/// Titre d'une page Notion : la propriété de type `title`, quel que soit son nom.
pub fn titre_page(v: &Value) -> String {
    if let Some(props) = v.get("properties").and_then(Value::as_object) {
        for p in props.values() {
            if p.get("type").and_then(Value::as_str) == Some("title") {
                let t = texte_riche(p.get("title").unwrap_or(&Value::Null));
                if !t.trim().is_empty() {
                    return t;
                }
            }
        }
    }
    v.get("title").and_then(Value::as_str).unwrap_or("").to_string()
}

/// Propriétés d'une page, en lignes « Nom : valeur » (le titre est déjà montré ailleurs).
pub fn proprietes_page(v: &Value) -> String {
    let Some(props) = v.get("properties").and_then(Value::as_object) else { return String::new() };
    // Triées par nom : d'une lecture à l'autre, les propriétés arrivent dans le même ordre.
    let mut paires: Vec<(&String, &Value)> = props.iter().collect();
    paires.sort_by(|a, b| a.0.cmp(b.0));
    let mut out = String::new();
    for (nom, p) in paires {
        if p.get("type").and_then(Value::as_str) == Some("title") {
            continue;
        }
        let val = valeur_propriete(p);
        if !val.is_empty() {
            out.push_str(&format!("{nom} : {val}\n"));
        }
    }
    out
}

/// La valeur d'une propriété, lisible. Ce qu'on ne sait pas lire est laissé vide plutôt
/// qu'affiché en JSON brut.
pub fn valeur_propriete(p: &Value) -> String {
    let tipe = p.get("type").and_then(Value::as_str).unwrap_or("");
    match tipe {
        "rich_text" | "title" => texte_riche(p.get(tipe).unwrap_or(&Value::Null)),
        "number" => p.get("number").and_then(Value::as_f64).map(|n| if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n}") }).unwrap_or_default(),
        "select" => p.pointer("/select/name").and_then(Value::as_str).unwrap_or("").to_string(),
        "multi_select" => p.pointer("/multi_select").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.get("name").and_then(Value::as_str)).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
        "status" => p.pointer("/status/name").and_then(Value::as_str).unwrap_or("").to_string(),
        "date" | "created_time" | "last_edited_time" => {
            let d = p.get("date").unwrap_or(&Value::Null);
            let debut = d.get("start").and_then(Value::as_str).or_else(|| p.get(tipe).and_then(Value::as_str)).unwrap_or("");
            match d.get("end").and_then(Value::as_str).filter(|e| !e.is_empty()) {
                Some(fin) => format!("{} → {}", heure_courte(debut), heure_courte(fin)),
                None => heure_courte(debut),
            }
        }
        "checkbox" => {
            if p.get("checkbox").and_then(Value::as_bool).unwrap_or(false) {
                t!("oui").to_string()
            } else {
                t!("non").to_string()
            }
        }
        "url" | "email" | "phone_number" => p.get(tipe).and_then(Value::as_str).unwrap_or("").to_string(),
        "people" => p.get("people").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.get("name").and_then(Value::as_str)).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
        "relation" => p.get("relation").and_then(Value::as_array).map(|a| t!("{} page(s) liée(s)", a.len())).unwrap_or_default(),
        "formula" => {
            let f = p.get("formula").unwrap_or(&Value::Null);
            match f.get("type").and_then(Value::as_str).unwrap_or("") {
                "string" => f.get("string").and_then(Value::as_str).unwrap_or("").to_string(),
                "number" => f.get("number").and_then(Value::as_f64).map(|n| if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n}") }).unwrap_or_default(),
                "checkbox" => if f.get("checkbox").and_then(Value::as_bool).unwrap_or(false) { t!("oui").to_string() } else { t!("non").to_string() },
                "date" => f.pointer("/date/start").and_then(Value::as_str).map(heure_courte).unwrap_or_default(),
                _ => String::new(),
            }
        }
        "unique_id" => match (p.pointer("/unique_id/prefix").and_then(Value::as_str), p.pointer("/unique_id/number").and_then(Value::as_i64)) {
            (Some(pre), Some(n)) => format!("{pre}-{n}"),
            (None, Some(n)) => format!("{n}"),
            _ => String::new(),
        },
        "files" => p.get("files").and_then(Value::as_array).map(|a| t!("{} fichier(s)", a.len())).unwrap_or_default(),
        "rollup" | "last_edited_by" | "created_by" | "verification" => String::new(),
        _ => String::new(),
    }
}

/// Une ligne de résultat : titre, type, adresse, propriétés utiles.
fn resume_objet(o: &Value) -> String {
    let tipe = o.get("object").and_then(Value::as_str).unwrap_or("");
    let icone = match tipe {
        "database" => "🗃",
        "page" => "📄",
        _ => "•",
    };
    let titre = match tipe {
        "database" => texte_riche(o.get("title").unwrap_or(&Value::Null)),
        _ => titre_page(o),
    };
    let mut out = format!("{icone} {}", if titre.trim().is_empty() { t!("(sans titre)").to_string() } else { titre });
    if let Some(u) = o.get("url").and_then(Value::as_str).filter(|u| !u.is_empty()) {
        out.push_str(&format!("\n  {u}"));
    }
    if let Some(id) = o.get("id").and_then(Value::as_str) {
        out.push_str(&format!("\n  id : {id}"));
    }
    if let Some(t) = o.get("last_edited_time").and_then(Value::as_str).filter(|t| !t.is_empty()) {
        out.push_str(&format!("\n  {}", t!("modifié le {}", heure_courte(t))));
    }
    if tipe == "page" {
        let props = proprietes_page(o);
        for l in props.lines().take(6) {
            out.push_str(&format!("\n  {l}"));
        }
    }
    out.push('\n');
    out
}

/// `2026-11-16T09:30:00.000Z` → `2026-11-16 09:30` : assez pour s'y retrouver, sans le bruit.
pub fn heure_courte(t: &str) -> String {
    match (t.get(0..10), t.get(11..16)) {
        (Some(j), Some(h)) if !h.is_empty() => format!("{j} {h}"),
        (Some(j), _) => j.to_string(),
        _ => t.to_string(),
    }
}

// ── Nom et filtre de propriété ───────────────────────────────────────────────

/// Nom exact d'une propriété à partir d'un nom approximatif : Notion distingue les majuscules
/// et le modèle ne les devine pas toujours. On cherche l'égalité, puis l'insensible à la casse.
pub fn nom_propriete(objet: &Value, demande: &str) -> String {
    let Some(props) = objet.pointer("/properties").and_then(Value::as_object) else { return demande.to_string() };
    let d = demande.trim();
    if props.contains_key(d) {
        return d.to_string();
    }
    if let Some(n) = props.keys().find(|k| k.eq_ignore_ascii_case(d)) {
        return n.clone();
    }
    // Un seul mot demandé, une seule propriété qui le contient : pas d'ambiguïté possible.
    let dl = d.to_lowercase();
    let mut candidats: Vec<&String> = props.keys().filter(|k| k.to_lowercase().contains(&dl)).collect();
    if candidats.len() == 1 {
        return candidats.remove(0).clone();
    }
    d.to_string()
}

/// Filtre de requête de base, d'après le VRAI type de la propriété. `None` : type qu'on ne
/// sait pas filtrer (mieux vaut le dire que d'envoyer un filtre que Notion refusera).
pub fn filtre_propriete(nom: &str, tipe: &str, valeur: &str) -> Option<Value> {
    let v = valeur.trim();
    // Chaque type écrit au clair : la clé Notion est le nom du type, et l'opérateur dépend de
    // ce type (`contains` pour du texte, `equals` pour un choix, un booléen nu pour une case).
    Some(match tipe {
        "rich_text" => json!({"property": nom, "rich_text": {"contains": v}}),
        "title" => json!({"property": nom, "title": {"contains": v}}),
        "select" => json!({"property": nom, "select": {"equals": v}}),
        "status" => json!({"property": nom, "status": {"equals": v}}),
        "multi_select" => json!({"property": nom, "multi_select": {"contains": v}}),
        "url" => json!({"property": nom, "url": {"equals": v}}),
        "email" => json!({"property": nom, "email": {"equals": v}}),
        "phone_number" => json!({"property": nom, "phone_number": {"equals": v}}),
        "checkbox" => json!({"property": nom, "checkbox": coche(v)}),
        // Un nombre qui n'en est pas un : pas de filtre inventé, l'outil le dira.
        "number" => json!({"property": nom, "number": {"equals": v.parse::<f64>().ok()?}}),
        "date" => json!({"property": nom, "date": {"on_or_before": v}}),
        _ => return None,
    })
}

/// Une case à cocher dite en français, en anglais ou en booléen JSON.
fn coche(v: &str) -> bool {
    matches!(v, "true" | "oui" | "vrai" | "coché" | "coche")
}

/// Valeur à écrire, au format Notion, d'après le type réel de la propriété.
pub fn propriete_notion(tipe: &str, valeur: &str) -> Option<Value> {
    let v = valeur.trim();
    // Même raison qu'à la lecture : la clé Notion est le nom du type, écrite au clair.
    Some(match tipe {
        "rich_text" => json!({"rich_text": [{"text": {"content": v}}]}),
        "title" => json!({"title": [{"text": {"content": v}}]}),
        "select" => json!({"select": {"name": v}}),
        "status" => json!({"status": {"name": v}}),
        "multi_select" => json!({"multi_select": v.split(',').map(str::trim).filter(|x| !x.is_empty()).map(|x| json!({"name": x})).collect::<Vec<_>>()}),
        "checkbox" => json!({"checkbox": coche(v)}),
        "number" => json!({"number": v.parse::<f64>().ok()?}),
        "date" => json!({"date": {"start": v}}),
        "url" => json!({"url": v}),
        "email" => json!({"email": v}),
        "phone_number" => json!({"phone_number": v}),
        _ => return None,
    })
}

// ── Markdown → blocs Notion ──────────────────────────────────────────────────

/// Un bloc Notion dont le corps porte le nom du type : `{"object":"block","type":T, T:{…}}`.
/// Bâti à la main parce que la clé du corps dépend du type : autant ne pas faire calculer une
/// clé d'objet par la macro `json!`.
fn bloc_avec(tipe: &str, corps: Value) -> Value {
    let mut b = serde_json::Map::new();
    b.insert("object".into(), json!("block"));
    b.insert("type".into(), json!(tipe));
    b.insert(tipe.into(), corps);
    Value::Object(b)
}

/// Un bloc de texte simple : paragraphe, titre, élément de liste, citation.
fn bloc_texte(tipe: &str, texte: &str) -> Value {
    bloc_avec(tipe, json!({"rich_text": [{"type": "text", "text": {"content": texte}}]}))
}

/// Le texte d'un bloc, dans la forme que Notion attend.
fn texte_brut(texte: &str) -> Value {
    json!([{"type": "text", "text": {"content": texte}}])
}

/// Notion veut un nom de sa liste de langues ; une clôture ``` nue donne « plain text ».
fn nom_langue(l: &str) -> String {
    if l.trim().is_empty() { "plain text".to_string() } else { l.trim().to_string() }
}

/// Markdown → blocs Notion. Ce que L Katta sait écrire dans sa bulle (titres, listes, code,
/// citations, cases à cocher, séparateurs) passe ; le reste devient des paragraphes.
///
/// Entre deux clôtures ```, tout est du texte brut : un `# commentaire` dans du code ne doit
/// pas devenir un titre.
pub fn blocs_depuis_markdown(md: &str) -> Vec<Value> {
    let mut out: Vec<Value> = vec![];
    // `Some((langue, lignes))` tant qu'on est dans un bloc de code ouvert.
    let mut code: Option<(String, Vec<String>)> = None;
    for l in md.lines() {
        // `take()` sort l'accumulateur : on peut le refermer ou le rendre sans conflit d'emprunt.
        if let Some((langue, mut corps)) = code.take() {
            if l.trim_start().starts_with("```") {
                out.push(bloc_avec("code", json!({"language": nom_langue(&langue), "rich_text": texte_brut(&corps.join("\n"))})));
            } else {
                corps.push(l.to_string());
                code = Some((langue, corps));
            }
            continue;
        }
        if let Some(reste) = l.trim_start().strip_prefix("```") {
            code = Some((reste.trim().to_string(), vec![]));
            continue;
        }
        let t = l.trim();
        if t.is_empty() {
            continue;
        }
        if t == "---" || t == "***" || t == "___" {
            out.push(bloc_avec("divider", json!({})));
            continue;
        }
        // Du plus précis au plus large : « ### » avant « ## » avant « # ».
        if let Some(h) = t.strip_prefix("### ") {
            out.push(bloc_texte("heading_3", h.trim()));
            continue;
        }
        if let Some(h) = t.strip_prefix("## ") {
            out.push(bloc_texte("heading_2", h.trim()));
            continue;
        }
        if let Some(h) = t.strip_prefix("# ") {
            out.push(bloc_texte("heading_1", h.trim()));
            continue;
        }
        if let Some(q) = t.strip_prefix("> ") {
            out.push(bloc_texte("quote", q.trim()));
            continue;
        }
        // « - [ ] à faire » et « - [x] fait ». L'espace après « [ » compte : sans `trim_start`,
        // une case à décocher retombait en paragraphe. Un « - [ » qui n'est pas une case
        // (pas de « ] ») reste du texte, il n'est pas deviné.
        if let Some(r) = t.strip_prefix("- [") {
            let r = r.trim_start();
            let (fait, texte) = match r.strip_prefix("x] ").or_else(|| r.strip_prefix("X] ")) {
                Some(reste) => (true, reste),
                None => match r.strip_prefix("] ") {
                    Some(reste) => (false, reste),
                    None => {
                        out.push(bloc_texte("paragraph", t));
                        continue;
                    }
                },
            };
            out.push(bloc_avec("to_do", json!({"checked": fait, "rich_text": texte_brut(texte.trim())})));
            continue;
        }
        if let Some(i) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            out.push(bloc_texte("bulleted_list_item", i.trim()));
            continue;
        }
        // « 1. », « 2. »… : une liste numérotée, peu importe le numéro écrit.
        if let Some(i) = t.split_once(". ").filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).map(|(_, r)| r) {
            out.push(bloc_texte("numbered_list_item", i.trim()));
            continue;
        }
        out.push(bloc_texte("paragraph", t));
    }
    // Une clôture jamais refermée : le texte accumulé reste un bloc de code, pas perdu.
    if let Some((langue, corps)) = code {
        out.push(bloc_avec("code", json!({"language": nom_langue(&langue), "rich_text": texte_brut(&corps.join("\n"))})));
    }
    out
}

/// Aperçu d'un Markdown destiné à Notion, pour la demande d'accord : l'utilisateur voit ce
/// qui va partir, jamais seulement « envoyer dans Notion ».
pub fn apercu_ecriture(titre: &str, contenu: &str, cible: &str) -> String {
    let n = blocs_depuis_markdown(contenu).len();
    let mut q = t!("Écrire dans Notion ({}) ?", cible);
    if !titre.trim().is_empty() {
        q.push_str(&format!("\n{} {}", t!("Titre :"), titre.trim()));
    }
    q.push_str(&format!("\n{} {}", t!("Bloc(s) :"), n));
    q.push_str(&format!("\n\n{}", contenu.trim()));
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_from_every_url_shape() {
        let id = "1a2b3c4d-5e6f-7a8b-9c0d-1e2f3a4b5c6d";
        let plat = "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d";
        assert_eq!(id_depuis_url(plat), id);
        assert_eq!(id_depuis_url(id), id);
        assert_eq!(id_depuis_url(&format!("https://www.notion.so/{plat}")), id);
        assert_eq!(id_depuis_url(&format!("https://www.notion.so/Mon-Titre-{plat}")), id);
        assert_eq!(id_depuis_url(&format!("https://www.notion.so/Mon-Titre-{plat}?pvs=4")), id);
        assert_eq!(id_depuis_url(&format!("https:// equipe.notion.site/Titre/{id}")), id);
        assert_eq!(id_depuis_url(&format!("  {plat}  ")), id);
        // Un titre seul n'est pas un identifiant : on ne devine rien.
        assert_eq!(id_depuis_url("https://www.notion.so/Mon-Titre"), "");
        assert_eq!(id_depuis_url(""), "");
    }

    #[test]
    fn token_shape() {
        assert!(jeton_plausible(&format!("ntn_{}", "a".repeat(50))));
        assert!(jeton_plausible(&format!("secret_{}", "b".repeat(50))));
        assert!(!jeton_plausible("ntn_court"));
        assert!(!jeton_plausible(&format!("sk-or-{}", "c".repeat(50))));
        assert!(!jeton_plausible(&format!("ntn_{} avec espace", "d".repeat(50))));
    }

    #[test]
    fn rich_text_keeps_marks_and_moves_spaces_out() {
        let v = json!([
            {"plain_text": "bon ", "text": {"content": "bon "}, "annotations": {}},
            {"plain_text": " gras ", "text": {"content": " gras "}, "annotations": {"bold": true}},
            {"plain_text": "fin", "text": {"content": "fin"}, "annotations": {}}
        ]);
        assert_eq!(texte_riche(&v), "bon **gras** fin");
    }

    #[test]
    fn rich_text_link_and_code() {
        let v = json!([{"plain_text": "doc", "text": {"content": "doc"}, "href": "https://x.example", "annotations": {"code": true}}]);
        assert_eq!(texte_riche(&v), "[`doc`](https://x.example)");
        assert_eq!(texte_riche(&json!([])), "");
        assert_eq!(texte_riche(&Value::Null), "");
    }

    #[test]
    fn blocks_become_markdown() {
        let txt = |s: &str| json!([{"plain_text": s, "text": {"content": s}}]);
        assert_eq!(bloc_markdown(&json!({"type": "heading_2", "heading_2": {"rich_text": txt("Titre")}})), "## Titre");
        assert_eq!(bloc_markdown(&json!({"type": "bulleted_list_item", "bulleted_list_item": {"rich_text": txt("item")}})), "- item");
        assert_eq!(bloc_markdown(&json!({"type": "to_do", "to_do": {"checked": true, "rich_text": txt("fait")}})), "- [x] fait");
        assert_eq!(bloc_markdown(&json!({"type": "to_do", "to_do": {"checked": false, "rich_text": txt("a faire")}})), "- [ ] a faire");
        assert_eq!(bloc_markdown(&json!({"type": "divider", "divider": {}})), "---");
        assert_eq!(bloc_markdown(&json!({"type": "code", "code": {"language": "rust", "rich_text": txt("fn main() {}")}})), "```rust\nfn main() {}\n```");
        assert_eq!(bloc_markdown(&json!({"type": "callout", "callout": {"icon": {"emoji": "⚠️"}, "rich_text": txt("attention")}})), "> ⚠️ attention");
        assert_eq!(bloc_markdown(&json!({"type": "child_page", "child_page": {"title": "Sous-page"}})), "📄 Sous-page");
        // Un bloc inconnu est annoncé, jamais inventé.
        assert!(bloc_markdown(&json!({"type": "machin", "machin": {}})).contains("machin"));
    }

    #[test]
    fn page_title_is_found_whatever_its_name() {
        let p = json!({"properties": {"Nom": {"type": "title", "title": [{"plain_text": "Ma page", "text": {"content": "Ma page"}}]}}});
        assert_eq!(titre_page(&p), "Ma page");
        assert_eq!(titre_page(&json!({"title": "base"})), "base");
        assert_eq!(titre_page(&json!({})), "");
    }

    #[test]
    fn property_values_are_readable() {
        assert_eq!(valeur_propriete(&json!({"type": "select", "select": {"name": "En cours"}})), "En cours");
        assert_eq!(valeur_propriete(&json!({"type": "multi_select", "multi_select": [{"name": "a"}, {"name": "b"}]})), "a, b");
        assert_eq!(valeur_propriete(&json!({"type": "number", "number": 42.0})), "42");
        assert_eq!(valeur_propriete(&json!({"type": "date", "date": {"start": "2026-11-16T09:30:00.000Z"}})), "2026-11-16 09:30");
        assert_eq!(valeur_propriete(&json!({"type": "unique_id", "unique_id": {"prefix": "TACH", "number": 12}})), "TACH-12");
        // Ce qu'on ne sait pas lire reste vide plutôt que du JSON brut.
        assert_eq!(valeur_propriete(&json!({"type": "rollup", "rollup": {}})), "");
    }

    #[test]
    fn property_name_is_matched_case_insensitively() {
        let db = json!({"properties": {"Statut": {"type": "status"}, "Nom": {"type": "title"}}});
        assert_eq!(nom_propriete(&db, "Statut"), "Statut");
        assert_eq!(nom_propriete(&db, "statut"), "Statut");
        // Aucune propriété ne ressemble : on rend la demande telle quelle, Notion expliquera.
        assert_eq!(nom_propriete(&db, "Inconnu"), "Inconnu");
        assert_eq!(nom_propriete(&Value::Null, "x"), "x");
    }

    #[test]
    fn filters_and_values_follow_the_real_type() {
        assert_eq!(filtre_propriete("Nom", "rich_text", "rapport"), Some(json!({"property": "Nom", "rich_text": {"contains": "rapport"}})));
        assert_eq!(filtre_propriete("Fait", "checkbox", "oui"), Some(json!({"property": "Fait", "checkbox": true})));
        assert_eq!(filtre_propriete("Fait", "checkbox", "non"), Some(json!({"property": "Fait", "checkbox": false})));
        // Un nombre qui n'en est pas : pas de filtre inventé.
        assert!(filtre_propriete("Prix", "number", "beaucoup").is_none());
        assert!(filtre_propriete("X", "rollup", "v").is_none());
        assert_eq!(propriete_notion("multi_select", "a, b"), Some(json!({"multi_select": [{"name": "a"}, {"name": "b"}]})));
        assert!(propriete_notion("rollup", "v").is_none());
    }

    #[test]
    fn a_checkbox_said_in_any_of_those_ways() {
        for oui in ["true", "oui", "vrai", "coché", "coche"] {
            assert!(coche(oui), "{oui} devrait cocher");
            assert_eq!(propriete_notion("checkbox", oui), Some(json!({"checkbox": true})));
        }
        for non in ["false", "non", "faux", "", "peut-être"] {
            assert!(!coche(non), "{non} ne devrait pas cocher");
        }
    }

    #[test]
    fn a_tick_box_needs_the_space_after_the_bracket() {
        // C'est le cas qui tombait en paragraphe : « - [ ] » a un espace entre [ et ].
        let b = blocs_depuis_markdown("- [ ] à faire");
        assert_eq!(b.len(), 1);
        assert_eq!(b[0]["type"], "to_do");
        assert_eq!(b[0].pointer("/to_do/checked"), Some(&json!(false)));
        assert_eq!(b[0].pointer("/to_do/rich_text/0/text/content"), Some(&json!("à faire")));
        // Un « - [ » qui n'est pas une case n'est pas deviné : il reste du texte.
        let pas = blocs_depuis_markdown("- [note] importante");
        assert_eq!(pas[0]["type"], "paragraph");
    }

    #[test]
    fn blocks_carry_their_type_as_the_body_key() {
        // Notion attend {"type":T, T:{…}} : une clé mal placée et tout l'envoi est refusé.
        let b = blocs_depuis_markdown("## Titre");
        assert_eq!(b[0]["object"], "block");
        assert_eq!(b[0]["type"], "heading_2");
        assert!(b[0].get("heading_2").is_some(), "{}", b[0]);
        assert_eq!(b[0].pointer("/heading_2/rich_text/0/text/content"), Some(&json!("Titre")));
        // Une clôture ``` nue reçoit une langue que Notion accepte.
        assert_eq!(nom_langue(""), "plain text");
        assert_eq!(nom_langue("  rust  "), "rust");
    }

    #[test]
    fn markdown_becomes_blocks() {
        let blocs = blocs_depuis_markdown("# Titre\n\nUn paragraphe.\n\n- un\n- deux\n\n1. premier\n\n- [ ] à faire\n- [x] fait\n\n> cité\n\n---\n");
        let types: Vec<&str> = blocs.iter().filter_map(|b| b.get("type").and_then(Value::as_str)).collect();
        assert_eq!(types, ["heading_1", "paragraph", "bulleted_list_item", "bulleted_list_item", "numbered_list_item", "to_do", "to_do", "quote", "divider"]);
        assert_eq!(blocs[5].pointer("/to_do/checked"), Some(&json!(false)));
        assert_eq!(blocs[6].pointer("/to_do/checked"), Some(&json!(true)));
    }

    #[test]
    fn code_fences_survive_and_an_unclosed_one_is_kept() {
        let blocs = blocs_depuis_markdown("```rust\nfn a() {}\nlet x = 1;\n```\n");
        assert_eq!(blocs.len(), 1);
        assert_eq!(blocs[0].pointer("/code/language"), Some(&json!("rust")));
        assert_eq!(blocs[0].pointer("/code/rich_text/0/text/content"), Some(&json!("fn a() {}\nlet x = 1")));
        // Dedans d'une clôture, un « # » n'est pas un titre.
        let dedans = blocs_depuis_markdown("```\n# pas un titre\n");
        assert_eq!(dedans.len(), 1);
        assert_eq!(dedans[0]["type"], "code");
    }

    #[test]
    fn round_trip_markdown_blocks_markdown() {
        let md = "## Sous-titre\n- élément\n- [ ] tâche";
        let blocs = blocs_depuis_markdown(md);
        let rendu: Vec<String> = blocs.iter().map(bloc_markdown).collect();
        assert_eq!(rendu, ["## Sous-titre", "- élément", "- [ ] tâche"]);
    }

    #[test]
    fn empty_markdown_makes_no_blocks() {
        assert!(blocs_depuis_markdown("").is_empty());
        assert!(blocs_depuis_markdown("\n\n   \n").is_empty());
    }

    #[test]
    fn short_hours_and_indent() {
        assert_eq!(heure_courte("2026-11-16T09:30:00.000Z"), "2026-11-16 09:30");
        assert_eq!(heure_courte("2026-11-16"), "2026-11-16");
        assert_eq!(heure_courte(""), "");
        assert_eq!(indenter("a\n\nb", "  "), "  a\n\n  b");
        assert_eq!(indenter("a", ""), "a");
    }

    #[test]
    fn writing_preview_shows_what_leaves_the_machine() {
        let a = apercu_ecriture("Rapport", "# Titre\n- un\n- deux", "base Tâches");
        assert!(a.contains("Rapport") && a.contains("base Tâches"));
        // Le nombre de blocs et le texte lui-même sont montrés, pas seulement un titre.
        assert!(a.contains("2") && a.contains("- deux"));
    }

    #[test]
    fn errors_never_leak_the_token() {
        let jeton = format!("ntn_{}", "z".repeat(60));
        for e in [erreur_reseau(&ureq::Error::ConnectionFailed), erreur_api(401, &json!({})), erreur_api(404, &json!({})), erreur_api(500, &json!({"message": "boom"})), raison(&ureq::Error::ConnectionFailed)] {
            assert!(!e.contains(&jeton), "fuite possible : {e}");
        }
        assert!(erreur_api(401, &json!({})).to_lowercase().contains("jeton"));
        assert!(erreur_api(429, &json!({})).contains("attendre"));
    }
}
