//! La fenêtre « Ouvrir » du bureau, là où le portail freedesktop n'existe pas.
//!
//! Sous Linux, `portail.rs` ouvre `org.freedesktop.portal.FileChooser` par D-Bus : la fenêtre
//! habituelle du système. Ici il n'y a ni D-Bus ni portail, et Windows a sa propre fenêtre :
//! `GetOpenFileNameW`, dans `comdlg32`, qui est une bibliothèque du système. Aucune dépendance
//! nouvelle, comme le reste du backend Win32 (voir `docs/PLAN.md` §10).
//!
//! ## Pourquoi `GetOpenFileNameW` et pas `IFileOpenDialog`
//!
//! `IFileOpenDialog` est la boîte de dialogue moderne, celle qu'ouvrent la plupart des
//! applications récentes. L'écrire sans la crate `windows` veut dire dérouler à la main une
//! vtable COM : `CoCreateInstance` sur `CLSID_FileOpenDialog`, puis appeler les méthodes par leur
//! rang dans la table — `IUnknown` (3), `IModalWindow` (1), `IFileDialog` (une vingtaine), et
//! seulement ensuite `IFileOpenDialog`. Un rang décalé d'un cran ne se voit pas à la compilation,
//! qui ne vérifie que les types Rust et jamais l'ABI : on appelle une autre méthode, avec les
//! arguments d'une autre, et le résultat est imprévisible plutôt qu'erroné.
//!
//! `GetOpenFileNameW` est une fonction ordinaire avec une structure ordinaire, `OPENFILENAMEW`,
//! dont les champs sont dans un ordre fixé depuis des décennies et dont `lStructSize` dit au
//! système quelle version on connaît. Tout le risque tient dans un ordre de champs qu'on peut
//! relire ligne à ligne. Pour une boîte de dialogue dont on attend « l'utilisateur choisit des
//! fichiers », c'est le bon échange.
//!
//! ## Ce que fait ce fichier sur les autres systèmes
//!
//! Le module est choisi par `cfg` pour toute cible qui n'est pas Linux, donc il est aussi celui
//! d'un macOS éventuel. `GetOpenFileNameW` n'y existe pas : la fonction rend une erreur explicite,
//! jamais une liste vide. Une liste vide se lirait « l'utilisateur n'a rien choisi », ce qui est
//! faux — il n'a jamais eu de fenêtre — et L Katta continuerait sans pièce jointe en croyant
//! l'utilisateur d'accord.

use std::path::PathBuf;

/// Ouvre la fenêtre « Ouvrir » du bureau ; renvoie les fichiers choisis (vide : annulé).
pub fn ouvrir(titre: &str) -> Result<Vec<PathBuf>, String> {
    #[cfg(windows)]
    return ouvrir_windows(titre);
    #[cfg(not(windows))]
    return Err(t!("la fenêtre « Ouvrir » du système n'est pas encore câblée ici").to_string());
}

// ---------------------------------------------------------------------------------------------
// Le découpage de la réponse et la construction du filtre.
//
// Ces deux fonctions sont du Rust pur : elles existent sur toutes les cibles, pour que les essais
// en bas du fichier vérifient le format de réponse partout — y compris là où la boîte de dialogue
// n'existe pas, donc là où personne ne pourrait le découvrir en l'utilisant. Le `allow` n'est là
// que pour ça : sans lui, la CI Linux, qui compile avec `-D warnings`, refuserait du code qu'elle
// ne peut pourtant pas exécuter.
// ---------------------------------------------------------------------------------------------

/// Décode le tampon de réponse.
///
/// Avec `OFN_ALLOWMULTISELECT`, Windows écrit soit **un** chemin complet (un seul fichier
/// choisi), soit un dossier suivi des noms choisis, chaque champ terminé par un NUL et la liste
/// entière par un double NUL. Les deux cas se lisent de la même façon : un seul morceau est un
/// chemin complet, plusieurs morceaux sont un dossier puis des noms.
#[cfg_attr(not(windows), allow(dead_code))]
fn decouper(tampon: &[u16]) -> Vec<PathBuf> {
    let mut morceaux: Vec<Vec<u16>> = Vec::new();
    let mut courant: Vec<u16> = Vec::new();
    for &c in tampon {
        if c == 0 {
            if courant.is_empty() {
                break; // Deux NUL de suite : la liste est finie, le reste du tampon n'est pas à nous.
            }
            morceaux.push(std::mem::take(&mut courant));
        } else {
            courant.push(c);
        }
    }
    if !courant.is_empty() {
        morceaux.push(courant); // Un tampon non terminé, au cas où.
    }
    match morceaux.as_slice() {
        [] => vec![],
        [seul] => vec![vers_chemin(seul)],
        [dossier, noms @ ..] => {
            let dossier = vers_chemin(dossier);
            noms.iter().map(|n| dossier.join(vers_chemin(n))).collect()
        }
    }
}

/// Un chemin depuis de l'UTF-16.
///
/// `OsString::from_wide` serait la conversion exacte, mais elle n'existe que sous Windows et
/// rendrait cette fonction — donc ses essais — inutilisable ailleurs. `from_utf16_lossy` ne perd
/// quelque chose que sur un chemin qui n'est pas de l'UTF-16 valide, ce qu'aucun système de
/// fichiers Windows ne produit en pratique.
#[cfg_attr(not(windows), allow(dead_code))]
fn vers_chemin(u: &[u16]) -> PathBuf {
    PathBuf::from(String::from_utf16_lossy(u))
}

/// Le filtre de la boîte de dialogue : paires « libellé, motif », chaque champ terminé par un NUL
/// et la liste entière terminée par un NUL de plus. Un NUL manquant fait afficher un filtre
/// tronqué, ou aucun.
#[cfg_attr(not(windows), allow(dead_code))]
fn filtre(paires: &[(&str, &str)]) -> Vec<u16> {
    let mut v: Vec<u16> = Vec::new();
    for (libelle, motif) in paires {
        v.extend(libelle.encode_utf16());
        v.push(0);
        v.extend(motif.encode_utf16());
        v.push(0);
    }
    v.push(0);
    v
}

/// Termine une chaîne par un NUL, comme l'API le demande partout.
#[cfg_attr(not(windows), allow(dead_code))]
fn avec_nul(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

// ---------------------------------------------------------------------------------------------
// L'appel lui-même.
// ---------------------------------------------------------------------------------------------

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    use super::OpenFileNameW;

    /// Boîte de dialogue au look actuel, plusieurs fichiers à la fois, fichiers et dossiers qui
    /// doivent exister, et sans changer le dossier courant du processus — ce dernier point compte :
    /// la boîte de dialogue appelle `SetCurrentDirectory` par défaut, et L Katta résout des chemins
    /// relatifs ailleurs.
    pub const DRAPEAUX: u32 = 0x0008_0000 // OFN_EXPLORER
        | 0x0000_0200 // OFN_ALLOWMULTISELECT
        | 0x0000_1000 // OFN_FILEMUSTEXIST
        | 0x0000_0800 // OFN_PATHMUSTEXIST
        | 0x0000_0008; // OFN_NOCHANGEDIR

    /// Appartement à fil unique : la boîte de dialogue charge des extensions du shell, qui sont du
    /// COM et supposent ce modèle.
    const COINIT_APARTMENTTHREADED: u32 = 0x2;

    /// Assez grand pour plusieurs chemins longs. 65 536 caractères font 128 Ko, alloués le temps
    /// de la boîte de dialogue.
    pub const TAILLE_TAMPON: usize = 65_536;

    #[link(name = "comdlg32")]
    unsafe extern "system" {
        fn GetOpenFileNameW(ofn: *mut OpenFileNameW) -> i32;
        fn CommDlgExtendedError() -> u32;
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserve: *const c_void, modele: u32) -> i32;
        fn CoUninitialize();
    }

    pub fn ouvrir(titre: &str) -> Result<Vec<std::path::PathBuf>, String> {
        // SAFETY : premier argument réservé (NULL), second le modèle d'appartement.
        let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED) };
        // S_OK (0) et S_FALSE (1) veulent dire « initialisé ici ou plus tôt, à défaire ».
        // RPC_E_CHANGED_MODE veut dire « déjà initialisé autrement » : on continue sans toucher au
        // compteur, sinon on déferait l'initialisation de quelqu'un d'autre.
        let a_defaire = hr >= 0;
        let resultat = dialogue(titre);
        if a_defaire {
            // SAFETY : CoInitializeEx a réussi plus haut, exactement une fois.
            unsafe { CoUninitialize() };
        }
        resultat
    }

    fn dialogue(titre: &str) -> Result<Vec<std::path::PathBuf>, String> {
        let filtre = super::filtre(&[(t!("Tous les fichiers"), "*.*")]);
        let titre = super::avec_nul(titre);
        let mut tampon = vec![0u16; TAILLE_TAMPON];

        let mut ofn = OpenFileNameW {
            // `lStructSize` dit au système quelle version de la structure on remplit : c'est ce
            // qui rend les champs de fin optionnels, et ce qui fait qu'une structure incomplète ne
            // serait pas lue au-delà.
            l_struct_size: std::mem::size_of::<OpenFileNameW>() as u32,
            // Aucune fenêtre propriétaire : cet appel peut venir d'un fil qui n'en a pas. La
            // boîte de dialogue s'ouvre quand même, seulement elle peut passer derrière une autre
            // fenêtre — préférable à ne pas s'ouvrir.
            hwnd_owner: 0,
            h_instance: 0,
            lpstr_filter: filtre.as_ptr(),
            lpstr_custom_filter: std::ptr::null_mut(),
            n_max_cust_filter: 0,
            // Les filtres sont numérotés à partir de 1 ; 0 n'en sélectionne aucun.
            n_filter_index: 1,
            lpstr_file: tampon.as_mut_ptr(),
            n_max_file: TAILLE_TAMPON as u32,
            lpstr_file_title: std::ptr::null_mut(),
            n_max_file_title: 0,
            // NULL : le dernier dossier utilisé, que Windows retient lui-même.
            lpstr_initial_dir: std::ptr::null(),
            lpstr_title: titre.as_ptr(),
            flags: DRAPEAUX,
            n_file_offset: 0,
            n_file_extension: 0,
            lpstr_def_ext: std::ptr::null(),
            l_cust_data: 0,
            // Pas de procédure de rappel : 0 et non un pointeur de fonction transformé, ce que le
            // compilateur refuserait de vérifier.
            lpfn_hook: 0,
            lp_template_name: std::ptr::null(),
            pv_reserved: std::ptr::null(),
            dw_reserved: 0,
            flags_ex: 0,
        };

        // SAFETY : `ofn` décrit exactement `tampon`, `filtre` et `titre`, qui vivent tous les
        // trois jusqu'à la fin de l'appel.
        if unsafe { GetOpenFileNameW(&mut ofn) } != 0 {
            return Ok(super::decouper(&tampon));
        }
        // SAFETY : lecture d'un code d'erreur, sans état à libérer.
        match unsafe { CommDlgExtendedError() } {
            // 0 veut dire « l'utilisateur a annulé » : ce n'est pas une erreur.
            0 => Ok(vec![]),
            code => Err(t!("la fenêtre « Ouvrir » du système a échoué (code {})", code)),
        }
    }
}

/// La structure de `GetOpenFileNameW`, dans l'ordre du SDK.
///
/// L'ordre est tout ce qui compte ici : `#[repr(C)]` reproduit exactement cet agencement, y
/// compris l'alignement des pointeurs sur 8 octets en x64, et `l_struct_size` est calculé avec
/// `size_of` plutôt qu'écrit à la main — donc si un champ est ajouté par erreur, la taille change
/// et le système le voit.
///
/// Les handles et le pointeur de procédure de rappel sont des `isize` : de la taille d'un
/// pointeur, et cela évite de convertir une fonction en entier, ce que le compilateur signale à
/// juste titre.
///
/// Beaucoup de ces champs ne sont jamais relus : ils sont remplis pour le système (`lpstrFilter`,
/// `flags`), ou écrits par lui en retour (`nFileOffset`, `nFileExtension`), ou réservés
/// (`pvReserved`). Les retirer décalerait tout ce qui suit — la structure est un contrat, pas un
/// choix. D'où le `allow`, qui n'est pas là pour masquer du code mort.
#[cfg(windows)]
#[allow(dead_code)]
#[repr(C)]
struct OpenFileNameW {
    l_struct_size: u32,
    hwnd_owner: isize,
    h_instance: isize,
    lpstr_filter: *const u16,
    lpstr_custom_filter: *mut u16,
    n_max_cust_filter: u32,
    n_filter_index: u32,
    lpstr_file: *mut u16,
    n_max_file: u32,
    lpstr_file_title: *mut u16,
    n_max_file_title: u32,
    lpstr_initial_dir: *const u16,
    lpstr_title: *const u16,
    flags: u32,
    n_file_offset: u16,
    n_file_extension: u16,
    lpstr_def_ext: *const u16,
    l_cust_data: isize,
    lpfn_hook: isize,
    lp_template_name: *const u16,
    // Les trois derniers n'existent que depuis Windows 2000 ; ils sont là parce que `lStructSize`
    // annonce la taille complète.
    pv_reserved: *const std::ffi::c_void,
    dw_reserved: u32,
    flags_ex: u32,
}

#[cfg(windows)]
fn ouvrir_windows(titre: &str) -> Result<Vec<PathBuf>, String> {
    win::ouvrir(titre)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un seul fichier choisi : le tampon contient son chemin complet.
    #[test]
    fn one_file_is_a_full_path() {
        let mut t: Vec<u16> = "C:\\docs\\note.txt".encode_utf16().collect();
        t.push(0);
        t.resize(64, 0);
        assert_eq!(decouper(&t), [PathBuf::from("C:\\docs\\note.txt")]);
    }

    /// Plusieurs fichiers : un dossier, puis les noms, puis le double NUL de fin.
    ///
    /// L'attente est construite avec `join`, comme le fait le code, et non écrite avec des
    /// antislashes : ces essais tournent sur la CI Linux, où `PathBuf::join` sépare avec `/`. Une
    /// attente écrite « C:\\docs\\a.txt » y échouerait alors même que le découpage est juste.
    /// Ce qui est vérifié, c'est la séparation en dossier + noms, leur nombre, et l'assemblage.
    #[test]
    fn several_files_are_a_folder_then_names() {
        let mut t: Vec<u16> = Vec::new();
        for champ in ["C:\\docs", "a.txt", "b.txt"] {
            t.extend(champ.encode_utf16());
            t.push(0);
        }
        t.push(0); // la liste est finie
        t.extend("SUITE".encode_utf16()); // ce qui suit ne doit pas être lu
        let dossier = PathBuf::from("C:\\docs");
        assert_eq!(decouper(&t), [dossier.join("a.txt"), dossier.join("b.txt")]);
    }

    /// Annulé : le tampon commence par un NUL, donc il n'y a aucun morceau.
    #[test]
    fn an_empty_buffer_means_nothing_was_chosen() {
        assert!(decouper(&[0u16; 32]).is_empty());
        assert!(decouper(&[]).is_empty());
    }

    /// Un tampon qui ne se termine pas ne doit ni planter ni inventer un morceau vide.
    #[test]
    fn an_unterminated_buffer_is_still_readable() {
        let t: Vec<u16> = "C:\\x\\y.md".encode_utf16().collect();
        assert_eq!(decouper(&t), [PathBuf::from("C:\\x\\y.md")]);
    }

    /// Les accents et les écritures non latines passent : la réponse est de l'UTF-16.
    #[test]
    fn accented_paths_survive() {
        let mut t: Vec<u16> = Vec::new();
        for champ in ["C:\\Été", "café.txt"] {
            t.extend(champ.encode_utf16());
            t.push(0);
        }
        t.push(0);
        // Même raison que ci-dessus : l'attente est assemblée comme le fait le code.
        assert_eq!(decouper(&t), [PathBuf::from("C:\\Été").join("café.txt")]);
    }

    /// Le filtre est une suite de paires terminées par un NUL, et la liste par un NUL de plus.
    #[test]
    fn the_filter_is_double_null_terminated() {
        let f = filtre(&[("Texte", "*.txt")]);
        let attendu: Vec<u16> = "Texte\0*.txt\0\0".encode_utf16().collect();
        assert_eq!(f, attendu);
        // Deux paires : chaque champ a son NUL, et la liste entière se termine par un NUL de plus.
        let f2 = filtre(&[("Texte", "*.txt"), ("Tout", "*.*")]);
        assert_eq!(f2.last(), Some(&0));
        assert_eq!(f2.iter().filter(|c| **c == 0).count(), 5);
    }

    #[test]
    fn avec_nul_ajoute_un_seul_nul() {
        assert_eq!(avec_nul("ab"), "ab\0".encode_utf16().collect::<Vec<u16>>());
        assert_eq!(avec_nul(""), vec![0u16]);
    }

    /// Sur les systèmes où la boîte de dialogue n'existe pas, l'erreur dit ce qui manque au lieu
    /// de faire croire que l'utilisateur n'a rien choisi.
    #[cfg(not(windows))]
    #[test]
    fn the_dialog_reports_it_is_missing_rather_than_returning_no_files() {
        let r = ouvrir("Choisir un fichier");
        assert!(r.is_err(), "le portail a prétendu réussir là où il n'y a pas de fenêtre");
        assert!(r.unwrap_err().contains("câblée"), "l'erreur devrait dire ce qui manque");
    }
}
