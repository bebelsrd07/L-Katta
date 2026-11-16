//! Socle commun aux systèmes.
//!
//! Ce qui diffère entre Linux et Windows est rangé ici, et nulle part ailleurs : le reste du
//! code appelle ces fonctions sans savoir sur quoi il tourne. Trois règles suivies partout :
//!
//! - **Aucune dépendance nouvelle.** `std::os::unix::*` et `std::os::windows::*` sont dans la
//!   bibliothèque standard, chacun derrière sa `cfg`. Le socle n'ajoute donc rien au
//!   `Cargo.lock` — ce qui compte, parce que ce fichier ne peut être régénéré qu'avec cargo en
//!   main, et la CI tourne en `--locked`.
//! - **Rien n'est deviné.** Ce qui n'a pas de sens sur un système est écrit dans son
//!   commentaire, plutôt que de réussir en silence pour une autre raison que celle attendue.
//! - **La protection réelle est dite.** Sur Windows les bits de mode POSIX n'existent pas : ce
//!   n'est pas une raison pour croire un fichier protégé. Chaque fonction dit d'où vient
//!   vraiment la protection sur ce système.

use std::path::Path;

// ── Droits sur les fichiers et dossiers ──────────────────────────────────────

/// Un fichier qui contient des données personnelles : son propriétaire seul peut le lire
/// (0600 sur Linux).
#[cfg(unix)]
pub fn restreindre_fichier(chemin: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(chemin, std::fs::Permissions::from_mode(0o600))
}

#[cfg(windows)]
pub fn restreindre_fichier(_: &Path) -> std::io::Result<()> {
    // Les bits de mode POSIX n'existent pas ici : `set_permissions` ne change que l'attribut
    // lecture seule, et n'empêcherait personne d'autre de lire le fichier.
    //
    // La protection vient de l'emplacement. `paths::data_dir()` range tout sous
    // `%LOCALAPPDATA%\lkatta`, dont l'ACL par défaut n'accorde l'accès qu'au compte qui l'a
    // créé (plus SYSTEM et les administrateurs) — c'est l'équivalent fonctionnel du 0600, et
    // c'est pour ça que ce socle n'a rien à poser. Le jour où L Katta écrirait des données
    // personnelles hors de ce dossier, il faudra ici une vraie ACL (`SetNamedSecurityInfo`)
    // plutôt qu'un `Ok(())`.
    Ok(())
}

/// Un dossier de L Katta : l'utilisateur seul peut y entrer (0700 sur Linux).
#[cfg(unix)]
pub fn restreindre_dossier(chemin: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(chemin, std::fs::Permissions::from_mode(0o700))
}

#[cfg(windows)]
pub fn restreindre_dossier(_: &Path) -> std::io::Result<()> {
    // Même raison que `restreindre_fichier` : l'ACL héritée de `%LOCALAPPDATA%` fait le travail.
    Ok(())
}

/// Restreint un fichier déjà ouvert (quand on l'a créé avant de savoir quoi en faire).
#[cfg(unix)]
pub fn restreindre_ouvert(f: &std::fs::File) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    f.set_permissions(std::fs::Permissions::from_mode(0o600))
}

#[cfg(windows)]
pub fn restreindre_ouvert(_: &std::fs::File) -> std::io::Result<()> {
    Ok(())
}

/// Rend un programme exécutable (0755 sur Linux) : une AppImage téléchargée, un `.exe` reçu
/// d'une mise à jour.
#[cfg(unix)]
pub fn rendre_executable(chemin: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(chemin, std::fs::Permissions::from_mode(0o755))
}

#[cfg(windows)]
pub fn rendre_executable(_: &Path) -> std::io::Result<()> {
    // Sur Windows, être exécutable n'est pas un droit mais une extension : `.exe`, `.bat`,
    // `.cmd` le sont par nature, et un fichier sans ces extensions ne le devient pas par un
    // changement de droits. Rien à poser — l'étape 2 du portage vérifie l'extension et la
    // signature à la place.
    Ok(())
}

/// `OpenOptions` qui crée directement un fichier réservé à son propriétaire.
///
/// Créé avec les bons droits dès l'ouverture plutôt que restreint après : entre les deux, un
/// autre compte de la machine pourrait lire ce qui vient d'être écrit.
#[cfg(unix)]
pub fn options_privees() -> std::fs::OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut o = std::fs::OpenOptions::new();
    o.mode(0o600);
    o
}

#[cfg(windows)]
pub fn options_privees() -> std::fs::OpenOptions {
    // Pas de mode à poser (voir `restreindre_fichier`) : la protection vient de l'emplacement.
    std::fs::OpenOptions::new()
}

// ── Cycle de vie des processus enfants ───────────────────────────────────────

/// L'enfant meurt avec son parent : un moniteur lancé par L Katta ne doit pas rester orphelin
/// à chaque relance.
#[cfg(target_os = "linux")]
pub fn mourir_avec_parent(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::unix::process::CommandExt;
    // SAFETY : prctl est sûr entre fork et exec (pas d'allocation, pas de verrou).
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        })
    }
}

/// Ailleurs sur Unix (BSD, macOS) : pas de `PR_SET_PDEATHSIG`, et `libc` n'est plus une
/// dépendance que pour Linux. Rien de posé, l'enfant survit.
#[cfg(all(unix, not(target_os = "linux")))]
pub fn mourir_avec_parent(cmd: &mut std::process::Command) -> &mut std::process::Command {
    cmd
}

#[cfg(windows)]
pub fn mourir_avec_parent(cmd: &mut std::process::Command) -> &mut std::process::Command {
    // L'équivalent exact est un Job Object fermé à la mort du parent
    // (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`), qui demande les liaisons Win32 — donc une
    // dépendance nouvelle, et le `Cargo.lock` ne peut pas être régénéré d'ici. C'est l'étape 2
    // du portage, où le même Job Object servira aussi à la garde mémoire.
    //
    // En attendant, l'enfant survit à L Katta sur Windows. Ce n'est pas laissé au hasard pour
    // autant : ce qui doit vraiment s'arrêter est arrêté explicitement par son appelant
    // (les moniteurs sont tués à la fermeture, pas seulement abandonnés).
    cmd
}

/// Détache l'enfant : il survit à L Katta. C'est le contraire de `mourir_avec_parent`, et ça
/// sert à un moteur local qu'on démarre depuis le panneau — fermer le panneau ne doit pas le
/// tuer.
#[cfg(target_os = "linux")]
pub fn detacher(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::unix::process::CommandExt;
    // SAFETY : setsid est sûr entre fork et exec (pas d'allocation, pas de verrou). Il donne à
    // l'enfant sa propre session, donc plus de terminal de contrôle non plus.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        })
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
pub fn detacher(cmd: &mut std::process::Command) -> &mut std::process::Command {
    cmd
}

#[cfg(windows)]
pub fn detacher(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    // `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` : l'enfant n'a ni console ni groupe de
    // signaux partagé avec L Katta, donc il survit à la fermeture — le rôle de `setsid` ici.
    // Les deux constantes viennent de `winbase.h` ; `creation_flags` est dans `std`, donc
    // toujours aucune dépendance nouvelle.
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
}

// ── Chemins et octets ────────────────────────────────────────────────────────

/// `OsString` depuis des octets bruts, sans exiger qu'ils soient de l'UTF-8 valide.
///
/// Sert au glisser-déposer, où une liste `file://…` arrive en octets et peut contenir un
/// chemin non UTF-8 — sur Linux c'est légal, un nom de fichier est une suite d'octets.
#[cfg(unix)]
pub fn osstring_depuis_octets(o: Vec<u8>) -> std::ffi::OsString {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(o)
}

#[cfg(windows)]
pub fn osstring_depuis_octets(o: Vec<u8>) -> std::ffi::OsString {
    // Les chemins Windows sont en UTF-16, mais ce qui arrive ici vient d'une liste d'URI en
    // octets : l'UTF-8 est donc bien le décodage à tenter. Un octet invalide devient le
    // caractère de remplacement plutôt qu'un chemin inventé — un chemin faux qui « existe à
    // peu près » ouvrirait le mauvais fichier.
    std::ffi::OsString::from(String::from_utf8_lossy(&o).into_owned())
}

// ── Se relancer ──────────────────────────────────────────────────────────────

/// Remplace L Katta par une nouvelle instance de lui-même. **Ne revient pas** : soit la
/// relance a marché, soit le processus s'arrête.
///
/// Sert à la garde mémoire, qui préfère repartir proprement plutôt que continuer dégradé.
#[cfg(unix)]
pub fn relancer(exe: &Path, args: &[String]) -> ! {
    use std::os::unix::process::CommandExt;
    // `exec()` remplace le processus en place : même PID, plus de double instance. Il ne
    // revient qu'en cas d'échec.
    let err = std::process::Command::new(exe).args(args).exec();
    eprintln!("lkatta : relance impossible ({err}), arrêt.");
    std::process::exit(1)
}

#[cfg(windows)]
pub fn relancer(exe: &Path, args: &[String]) -> ! {
    // Aucun appel Windows ne remplace un processus en place. On lance donc la nouvelle
    // instance, puis on se termine — dans cet ordre, pour qu'un échec au démarrage ne laisse
    // jamais L Katta complètement arrêté sans rien dire. Lâcher le `Child` ne le tue pas.
    match std::process::Command::new(exe).args(args).spawn() {
        Ok(_) => std::process::exit(0),
        Err(e) => {
            eprintln!("lkatta : relance impossible ({e}), arrêt.");
            std::process::exit(1)
        }
    }
}

// ── Politesse envers la machine ──────────────────────────────────────────────

/// Le fil courant passe en priorité minimale : processeur **et** disque.
///
/// Sert à l'indexation, qui tourne en arrière-plan : elle ne doit ni manger le processeur de
/// l'utilisateur, ni repousser ses écritures vers le disque.
#[cfg(target_os = "linux")]
pub fn fil_discret() {
    // SAFETY : gettid, setpriority et ioprio_set portent sur le fil courant, sans aliasing ni
    // allocation. Les codes de retour sont ignorés exprès : ne pas être discret n'est pas une
    // erreur, seulement moins poli.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, tid, 19);
        // ioprio_set(IOPRIO_WHO_PROCESS, tid, IOPRIO_CLASS_IDLE << 13)
        libc::syscall(libc::SYS_ioprio_set, 1, tid as libc::c_long, 3i64 << 13);
    }
}

#[cfg(not(target_os = "linux"))]
pub fn fil_discret() {
    // Sur Windows l'équivalent est `SetThreadPriority(THREAD_PRIORITY_LOWEST)` couplé à
    // `PROCESS_MODE_BACKGROUND_BEGIN`, qui demandent les liaisons Win32 — donc une dépendance
    // nouvelle, impossible à ajouter tant que le `Cargo.lock` ne peut pas être régénéré. C'est
    // l'étape 2 du portage. L'indexation reste exacte, seulement moins discrète.
    //
    // Rien non plus sur les autres Unix : `libc` n'est plus une dépendance que pour Linux.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Un dossier de test à soi : les tests tournent en parallèle, et `process::id()` seul ne
    /// suffirait pas à les séparer.
    fn dossier(qui: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("lkatta-os-{}-{qui}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn mode(chemin: &Path) -> u32 {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(chemin).unwrap().permissions().mode() & 0o777
        }
        // Ailleurs les bits POSIX n'existent pas : il n'y a rien à mesurer, et le test se
        // contente de vérifier que les appels ne cassent pas.
        #[cfg(not(unix))]
        {
            let _ = chemin;
            0
        }
    }

    #[test]
    fn private_files_end_up_readable_by_their_owner_only() {
        let d = dossier("fichier");
        let f = d.join("secret.txt");
        std::fs::write(&f, "x").unwrap();
        restreindre_fichier(&f).unwrap();
        #[cfg(unix)]
        assert_eq!(mode(&f), 0o600);

        rendre_executable(&f).unwrap();
        #[cfg(unix)]
        assert_eq!(mode(&f), 0o755);

        // Un fichier déjà ouvert se restreint aussi.
        let ouvert = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
        restreindre_ouvert(&ouvert).unwrap();
        #[cfg(unix)]
        assert_eq!(mode(&f), 0o600);

        restreindre_dossier(&d).unwrap();
        #[cfg(unix)]
        assert_eq!(mode(&d), 0o700);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn private_options_create_the_file_already_private() {
        // C'est tout l'intérêt d'`options_privees` : le fichier n'est jamais lisible par un
        // autre compte, pas même entre sa création et l'appel qui le restreint.
        let d = dossier("options");
        let f = d.join("neuf.txt");
        {
            let mut fichier = options_privees().write(true).create_new(true).open(&f).unwrap();
            writeln!(fichier, "x").unwrap();
        }
        #[cfg(unix)]
        assert_eq!(mode(&f), 0o600, "créé puis restreint : il y a une fenêtre où il est lisible");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_child_process_is_still_launchable_through_the_hal() {
        // Le socle ne doit pas empêcher de lancer quoi que ce soit : sur les systèmes où il ne
        // sait pas faire mourir l'enfant avec le parent, il rend la commande intacte.
        let mut cmd = std::process::Command::new(if cfg!(windows) { "cmd" } else { "true" });
        if cfg!(windows) {
            cmd.arg("/c").arg("exit 0");
        }
        mourir_avec_parent(&mut cmd);
        let sortie = cmd.output();
        assert!(sortie.is_ok(), "le socle a rendu la commande inutilisable : {sortie:?}");
    }
}
