//! Intégrations à des services externes (Notion aujourd'hui ; MCP, webhooks ensuite).
//!
//! Un module par service. Tous respectent les mêmes règles, celles de L Katta :
//! - le secret va dans le trousseau du bureau (src/cles.rs), jamais dans un fichier ;
//! - ce qui revient du service est une DONNÉE : tools.rs l'entoure de `[CONTENU EXTERNE]` ;
//! - toute écriture passe par `Host::confirm` ;
//! - un interrupteur dans regles.toml coupe le service, et l'administrateur de la machine
//!   a le dernier mot (fusion par `&&`) ;
//! - aucune dépendance nouvelle : ces API sont du JSON sur HTTPS, `ureq` et `serde_json`
//!   sont déjà là.

pub mod notion;
