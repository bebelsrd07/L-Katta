//! Le transport du protocole de commande, sous Windows : un tube nommé.
//!
//! Le protocole lui-même (les lignes de texte, leur analyse) est dans `ipc.rs`, qui est
//! portable. Ici il n'y a que le tuyau.
//!
//! Le tube s'appelle `\\.\pipe\lkatta-<session>`, où `<session>` est l'identifiant de session
//! Windows du processus. Une instance de tube par client, créée puis fermée à chaque connexion :
//! rien d'étatique à nettoyer, et un client lent ne retient pas les suivants.
//!
//! ## Pourquoi ce n'est pas un socket
//!
//! Un socket local (`127.0.0.1`) marcherait aussi et n'aurait demandé aucun appel Win32. Il a
//! deux défauts ici : il occupe un port, et le pare-feu Windows peut demander à l'utilisateur
//! s'il autorise l'application à écouter — une fenêtre inquiétante pour un compagnon de bureau
//! qui ne parle à personne. Le tube nommé ne touche pas au réseau.
//!
//! ## La vérification d'identité, dans les deux sens
//!
//! Un tube nommé n'est pas protégé par son nom : n'importe quel processus local peut en créer un
//! du même nom avant nous, ou s'y connecter. Deux attaques, donc deux vérifications.
//!
//! - **Le serveur vérifie le client.** Sans ça, un autre processus de la session pourrait
//!   envoyer `dire …` ou `quit` à la place de l'utilisateur.
//! - **Le client vérifie le serveur.** Sans ça, un tube usurpé recevrait les commandes tapées —
//!   y compris, via `lkatta`, des textes que l'utilisateur croit confidentiels.
//!
//! Les deux se font de la même façon : demander au tube le PID de l'autre bout, ouvrir son
//! jeton, lire son SID d'utilisateur, et le comparer au nôtre. **Refus en cas de doute** : si une
//! étape échoue — jeton illisible, PID absent — la connexion est rejetée, pas acceptée. Un
//! compagnon qui refuse de parler vaut mieux qu'un compagnon qui parle au mauvais processus.
//!
//! ## Ce qui est `unsafe` ici, et pourquoi c'est borné
//!
//! Les poignées sont enveloppées dans `std::fs::File` dès qu'elles sont obtenues : la lecture,
//! l'écriture et la fermeture passent alors par `std`, avec ses délais et ses erreurs habituels.
//! Le `unsafe` se limite à l'obtention de la poignée et à la comparaison des SID. Aucune
//! dépendance nouvelle : `kernel32` et `advapi32` sont des bibliothèques du système, et
//! `#[link]` dit seulement à l'éditeur de liens de les prendre.

use std::ffi::c_void;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::windows::io::{FromRawHandle, IntoRawHandle};
use std::ptr;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateNamedPipeW(
        nom: *const u16,
        acces: u32,
        mode: u32,
        instances: u32,
        taille_sortie: u32,
        taille_entree: u32,
        delai_defaut: u32,
        securite: *mut c_void,
    ) -> isize;
    fn CreateFileW(
        nom: *const u16,
        acces: u32,
        partage: u32,
        securite: *mut c_void,
        creation: u32,
        attributs: u32,
        modele: isize,
    ) -> isize;
    fn ConnectNamedPipe(tube: isize, chevauche: *mut c_void) -> i32;
    fn DisconnectNamedPipe(tube: isize) -> i32;
    fn FlushFileBuffers(h: isize) -> i32;
    fn CloseHandle(h: isize) -> i32;
    fn WaitNamedPipeW(nom: *const u16, delai: u32) -> i32;
    fn GetLastError() -> u32;
    fn GetCurrentProcessId() -> u32;
    fn ProcessIdToSessionId(pid: u32, session: *mut u32) -> i32;
    fn GetNamedPipeClientProcessId(tube: isize, pid: *mut u32) -> i32;
    fn GetNamedPipeServerProcessId(tube: isize, pid: *mut u32) -> i32;
    fn OpenProcess(acces: u32, herite: i32, pid: u32) -> isize;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: isize, acces: u32, jeton: *mut isize) -> i32;
    fn GetTokenInformation(jeton: isize, classe: i32, info: *mut c_void, taille: u32, rendu: *mut u32) -> i32;
    fn EqualSid(a: *const c_void, b: *const c_void) -> i32;
}

///Tube en accès lecture-écriture.
const ACCES_TUBE: u32 = 0x0000_0003; // PIPE_ACCESS_DUPLEX
/// Octets, lecture en octets, attente bloquante, et refus par le noyau de tout client distant
/// (SMB) : le tube n'est joignable que depuis cette machine.
const MODE_TUBE: u32 = 0x0000_0008; // PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS
/// `PIPE_UNLIMITED_INSTANCES`. Le fil serveur ferme chaque instance avant d'en créer une autre,
/// donc il n'y en a jamais qu'une en pratique — mais après `CloseHandle` le nom peut traîner un
/// instant, et une limite à 1 ferait alors échouer la création suivante sans raison.
const INSTANCES: u32 = 255;
const INVALIDE: isize = -1;
const ERREUR_TUBE_OCCUPE: u32 = 231; // ERROR_PIPE_BUSY
const ERREUR_DEJA_CONNECTE: u32 = 535; // ERROR_PIPE_CONNECTED
const OUVRIR_EXISTANT: u32 = 3; // OPEN_EXISTING
const LIRE_ET_ECRIRE: u32 = 0xC000_0000; // GENERIC_READ | GENERIC_WRITE
/// `PROCESS_QUERY_LIMITED_INFORMATION` : suffit pour ouvrir le jeton, et réussit sur plus de
/// processus que `PROCESS_QUERY_INFORMATION`, notamment ceux qui sont protégés.
const REQUETER: u32 = 0x1000;
const JETON_LIRE: u32 = 0x0008; // TOKEN_QUERY
const JETON_UTILISATEUR: i32 = 1; // TokenUser
/// Une commande tient dans 64 Ko, comme sous Linux : un client bavard ne doit pas pouvoir faire
/// grossir le démon sans limite.
const TAILLE_MAX: usize = 64 * 1024;

/// Nom du tube de cette session.
pub fn nom_du_tube() -> String {
    let mut session = 0u32;
    // SAFETY : écrit un u32 dans notre variable, et échoue proprement si le PID est inconnu.
    let ok = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) };
    // Session 0 si l'appel échoue : c'est la session des services. Le tube reste alors joignable
    // par cette session seule, et l'échec de l'appel ne doit pas empêcher le démon de démarrer.
    if ok == 0 {
        session = 0;
    }
    format!(r"\\.\pipe\lkatta-{session}")
}

fn large(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Le SID d'utilisateur du processus courant. `None` si Windows ne le donne pas — et dans ce cas
/// les connexions sont refusées, pas acceptées par défaut.
fn utilisateur_courant() -> Option<Sid> {
    // SAFETY : notre propre pseudo-poignée de processus (-1), toujours valide, en lecture seule.
    let pid = unsafe { GetCurrentProcessId() };
    sid_du_processus(pid)
}

/// Le SID d'utilisateur d'un processus quelconque de la session.
fn sid_du_processus(pid: u32) -> Option<Sid> {
    // SAFETY : ouverture en lecture seule d'un processus ; poignée nulle en cas d'échec, fermée
    // sur tous les chemins de retour.
    let processus = unsafe { OpenProcess(REQUETER, 0, pid) };
    if processus == 0 {
        return None;
    }
    let r = (|| {
        let mut jeton: isize = 0;
        // SAFETY : écrit une poignée de jeton dans notre variable.
        if unsafe { OpenProcessToken(processus, JETON_LIRE, &mut jeton) } == 0 {
            return None;
        }
        let _garde = GardeJeton(jeton);
        let mut rendu = 0u32;
        // SAFETY : premier appel volontairement nul pour demander la taille nécessaire ; Windows
        // écrit `rendu` et renvoie 0 avec ERROR_INSUFFICIENT_BUFFER.
        unsafe { GetTokenInformation(jeton, JETON_UTILISATEUR, ptr::null_mut(), 0, &mut rendu) };
        if rendu == 0 || rendu > 4096 {
            return None;
        }
        let mut tampon = vec![0u8; rendu as usize];
        // SAFETY : le tampon a exactement la taille annoncée par l'appel précédent.
        let ok = unsafe {
            GetTokenInformation(jeton, JETON_UTILISATEUR, tampon.as_mut_ptr() as *mut c_void, rendu, &mut rendu)
        };
        if ok == 0 {
            return None;
        }
        Some(Sid(tampon))
    })();
    // SAFETY : fermeture du jeton, ouverte plus haut, sur tous les chemins.
    unsafe { CloseHandle(processus) };
    r
}

/// Ferme le jeton en sortant du bloc, y compris en cas de retour anticipé : sans ça une poignée
/// fuirait à chaque connexion, et le démon tournerait longtemps.
struct GardeJeton(isize);

impl Drop for GardeJeton {
    fn drop(&mut self) {
        // SAFETY : poignée obtenue par OpenProcessToken, vivante, fermée une seule fois.
        unsafe { CloseHandle(self.0) };
    }
}

/// Le SID d'utilisateur, lu dans un `TOKEN_USER`.
///
/// Le tampon contient la structure **et** le SID lui-même, écrit à la suite par Windows : tant
/// que le tampon vit, le pointeur de SID reste valide. C'est pour ça que le tampon est gardé ici
/// plutôt que d'en extraire un pointeur nu.
struct Sid(Vec<u8>);

impl Sid {
    /// Pointeur vers le SID lui-même : premier champ de `TOKEN_USER`, donc premier mot du tampon.
    ///
    /// `None` si le tampon est trop court ou si le pointeur est nul. Ni l'un ni l'autre ne doit
    /// arriver, mais deviner une valeur ici voudrait dire comparer autre chose que des SID — et
    /// `EqualSid` avec un pointeur nul est indéfini, pas seulement faux.
    fn pointeur(&self) -> Option<*const c_void> {
        const MOT: usize = std::mem::size_of::<usize>();
        if self.0.len() < MOT {
            return None;
        }
        let mut octets = [0u8; MOT];
        octets.copy_from_slice(&self.0[..MOT]);
        let adresse = usize::from_ne_bytes(octets);
        (adresse != 0).then_some(adresse as *const c_void)
    }

    fn egal(&self, autre: &Sid) -> bool {
        // Un SID illisible des deux côtés ne fait pas « égaux » : il fait « inconnu », donc refusé.
        let (Some(a), Some(b)) = (self.pointeur(), autre.pointeur()) else { return false };
        // SAFETY : les deux tampons sont vivants (ils sont dans `self` et `autre`), les deux
        // pointeurs viennent de `GetTokenInformation` et viennent d'être vérifiés non nuls.
        unsafe { EqualSid(a, b) != 0 }
    }
}

/// Écoute le tube et lance un fil pour cela. `repondre` transforme une ligne reçue en la ligne
/// à renvoyer, terminaison comprise.
pub fn ecouter(repondre: impl Fn(&str) -> String + Send + 'static) -> io::Result<()> {
    ecouter_sur(&nom_du_tube(), repondre)
}

fn ecouter_sur(chemin: &str, repondre: impl Fn(&str) -> String + Send + 'static) -> io::Result<()> {
    let nom = large(chemin);
    let moi = match utilisateur_courant() {
        Some(s) => s,
        // Sans SID de référence on ne peut reconnaître personne : mieux vaut ne pas écouter du
        // tout que d'accepter n'importe quel processus de la session.
        None => return Err(io::Error::other("le SID de l'utilisateur courant est illisible")),
    };
    // Première instance créée avant de rendre la main : dès le retour d'`ecouter`, le tube existe
    // et un client immédiat ne tombe jamais entre deux fils.
    // SAFETY : création d'une instance de tube ; INVALID_HANDLE_VALUE en cas d'échec.
    let premier = unsafe {
        CreateNamedPipeW(nom.as_ptr(), ACCES_TUBE, MODE_TUBE, INSTANCES, TAILLE_MAX as u32, TAILLE_MAX as u32, 0, ptr::null_mut())
    };
    if premier == INVALIDE {
        return Err(io::Error::last_os_error());
    }
    let repondre = std::sync::Arc::new(std::sync::Mutex::new(repondre));
    std::thread::spawn(move || {
        let mut prochain = Some(premier);
        loop {
            let tube = match prochain.take() {
                Some(t) => t,
                None => unsafe {
                    CreateNamedPipeW(nom.as_ptr(), ACCES_TUBE, MODE_TUBE, INSTANCES, TAILLE_MAX as u32, TAILLE_MAX as u32, 0, ptr::null_mut())
                },
            };
            if tube == INVALIDE {
                // Plus de ressources, nom refusé : inutile d'insister en boucle serrée.
                return;
            }
            // SAFETY : attente bloquante d'un client sur notre instance fraîche.
            if unsafe { ConnectNamedPipe(tube, ptr::null_mut()) } == 0 {
                let e = unsafe { GetLastError() };
                // ERROR_PIPE_CONNECTED : un client s'est connecté avant l'appel. Ce n'est pas un
                // échec, et le traiter comme tel jetterait une connexion valide.
                if e != ERREUR_DEJA_CONNECTE {
                    fermer(tube);
                    continue;
                }
            }
            if !client_autorise(tube, &moi) {
                // SAFETY : coupe la connexion avant de fermer la poignée.
                unsafe { DisconnectNamedPipe(tube) };
                fermer(tube);
                continue;
            }
            // Un fil court par client : un client lent ne fige jamais les commandes suivantes.
            let cb = std::sync::Arc::clone(&repondre);
            std::thread::spawn(move || servir(tube, &cb));
        }
    });
    Ok(())
}

/// Le client au bout du tube est-il notre propre utilisateur ?
fn client_autorise(tube: isize, moi: &Sid) -> bool {
    let mut pid = 0u32;
    // SAFETY : écrit un u32 ; renvoie 0 si le tube ne connaît pas son client.
    if unsafe { GetNamedPipeClientProcessId(tube, &mut pid) } == 0 {
        return false;
    }
    match sid_du_processus(pid) {
        Some(s) => moi.egal(&s),
        // Jeton illisible : doute, donc refus.
        None => false,
    }
}

/// Lit une commande, écrit la réponse, puis ferme. Une instance de tube ne sert qu'une fois.
fn servir(tube: isize, repondre: &std::sync::Mutex<impl Fn(&str) -> String>) {
    // SAFETY : la poignée vient de CreateNamedPipeW et n'est possédée par rien d'autre ; le
    // `File` en prend la propriété, donc sa fermeture est la seule.
    let mut f = unsafe { std::fs::File::from_raw_handle(tube as *mut c_void) };
    let mut ligne = String::new();
    // `Take<&File>` ne sait que lire des octets : `read_line` vient de `BufRead`, donc il faut
    // l'envelopper. La borne de 64 Ko reste appliquée avant le tampon, comme sous Linux.
    let lu = BufReader::new((&f).take(TAILLE_MAX as u64)).read_line(&mut ligne).unwrap_or(0);
    // Une sonde `present()` referme sans rien envoyer : aucune commande à traiter ni à répondre.
    if lu > 0 && !ligne.trim().is_empty() {
        if let Ok(cb) = repondre.lock() {
            let rep = cb(&ligne);
            drop(cb);
            let _ = f.write_all(rep.as_bytes());
            let _ = f.flush();
            let h = f.into_raw_handle() as isize;
            // SAFETY : poignée valide, vidée puis déconnectée et fermée une seule fois.
            unsafe {
                FlushFileBuffers(h);
                DisconnectNamedPipe(h);
                CloseHandle(h);
            }
            return;
        }
    }
    let h = f.into_raw_handle() as isize;
    // SAFETY : poignée valide, déconnectée et fermée une seule fois.
    unsafe {
        DisconnectNamedPipe(h);
        CloseHandle(h);
    }
}

fn fermer(tube: isize) {
    // SAFETY : poignée venant de CreateNamedPipeW, fermée une seule fois.
    unsafe { CloseHandle(tube) };
}

/// Envoie une commande au démon et attend la réponse. Erreur si le démon ne tourne pas.
pub fn envoyer(ligne: &str) -> io::Result<String> {
    envoyer_sur(&nom_du_tube(), ligne)
}

fn envoyer_sur(chemin: &str, ligne: &str) -> io::Result<String> {
    let nom = large(chemin);
    let h = ouvrir(&nom)?;
    // SAFETY : poignée venant de CreateFileW, non possédée ailleurs ; le `File` la prend.
    let mut f = unsafe { std::fs::File::from_raw_handle(h as *mut c_void) };

    // Le serveur est-il bien notre propre utilisateur ? Un tube du même nom créé par un autre
    // processus de la session recevrait sinon la commande tapée.
    if !serveur_autorise(h) {
        return Err(io::Error::other("le tube du démon appartient à un autre utilisateur"));
    }

    f.write_all(ligne.replace('\n', " ").as_bytes())?;
    f.write_all(b"\n")?;
    f.flush()?;
    let mut out = String::new();
    BufReader::new(f).read_line(&mut out)?;
    Ok(out.trim().to_string())
}

fn serveur_autorise(tube: isize) -> bool {
    let (Some(moi), Some(pid)) = (utilisateur_courant(), {
        let mut pid = 0u32;
        // SAFETY : écrit un u32.
        (unsafe { GetNamedPipeServerProcessId(tube, &mut pid) } != 0).then_some(pid)
    }) else {
        return false;
    };
    match sid_du_processus(pid) {
        Some(s) => moi.egal(&s),
        None => false,
    }
}

fn ouvrir(nom: &[u16]) -> io::Result<isize> {
    // SAFETY : ouverture du tube ; INVALID_HANDLE_VALUE en cas d'échec.
    let h = unsafe { CreateFileW(nom.as_ptr(), LIRE_ET_ECRIRE, 0, ptr::null_mut(), OUVRIR_EXISTANT, 0, 0) };
    if h != INVALIDE {
        return Ok(h);
    }
    // Tube occupé : le démon sert un client à la fois, et une instance est recréée à chaque
    // connexion. C'est le seul code d'erreur qui mérite d'être retenté.
    if unsafe { GetLastError() } == ERREUR_TUBE_OCCUPE {
        // SAFETY : attente bornée à 3 s, sur un nom que nous venons de construire.
        if unsafe { WaitNamedPipeW(nom.as_ptr(), 3000) } != 0 {
            let h = unsafe { CreateFileW(nom.as_ptr(), LIRE_ET_ECRIRE, 0, ptr::null_mut(), OUVRIR_EXISTANT, 0, 0) };
            if h != INVALIDE {
                return Ok(h);
            }
        }
    }
    Err(io::Error::last_os_error())
}

/// Le démon est-il là ? On essaie de se connecter, voilà tout.
pub fn present() -> bool {
    present_sur(&nom_du_tube())
}

fn present_sur(chemin: &str) -> bool {
    match ouvrir(&large(chemin)) {
        Ok(h) => {
            // Se connecter ne suffit pas à dire que c'est bien notre démon : un tube usurpé
            // s'ouvrirait aussi. La vérification du serveur fait partie de la réponse.
            let ok = serveur_autorise(h);
            // SAFETY : poignée ouverte ici, fermée une seule fois.
            unsafe { CloseHandle(h) };
            ok
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_pipe_verifies_sid_and_exchanges_messages() {
        let chemin = format!(r"\\.\pipe\lkatta-test-{}", std::process::id());
        assert!(!present_sur(&chemin));
        ecouter_sur(&chemin, |l| format!("reçu:{}\n", l.trim())).expect("écoute du tube");
        assert!(present_sur(&chemin));
        // Laisse au fil serveur une milliseconde pour recréer l'instance après la sonde.
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(envoyer_sur(&chemin, "salut maki").unwrap(), "reçu:salut maki");
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(envoyer_sur(&chemin, "coucou\ndeux").unwrap(), "reçu:coucou deux");
    }
}
