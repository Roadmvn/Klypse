# Audit produit Klypse — 8 septembre 2026

La priorité est de fiabiliser le parcours **capturer → retoucher → retrouver → partager**, puis d'améliorer les gestes fréquents. Les quatre défauts prioritaires de l'audit ont maintenant reçu des correctifs locaux. Ce document distingue le constat initial, le comportement corrigé et les validations effectivement réalisées.

Périmètre : copie de travail locale, comprenant les modifications non commitées après `bffdc53`. L'audit initial était une analyse du code et de l'interface, complétée par deux essais dans un bureau Xvfb/Xfwm isolé. Les corrections décrites ci-dessous ont été réalisées ensuite, à la demande de l'utilisateur. Elles ne constituent pas encore une version publiée sur GitHub.

Les validations automatisées et les essais X11 isolés ne sont pas présentés comme des essais sur tous les bureaux Linux. Les essais X11 précédents de cette session couvrent capture de menu, sélection par clic, découpage et masquage de la galerie pendant les captures ; ils ne prouvent pas le fonctionnement Wayland ou multiécran. Le réglage du délai, son affichage et son annulation ont été explicitement reportés par l'utilisateur.

## Corrections réalisées après l'audit

### 1. Partager exactement les retouches enregistrées — corrigé et testé

**Constat initial.** Après « Enregistrer » dans l'éditeur, les annotations et la miniature changeaient, mais la copie depuis la galerie et le glisser-déposer relisaient l'original. L'icône du glisser-déposer pouvait donc montrer une image retouchée alors que les pixels et le fichier transmis ne l'étaient pas.

**Correction.** La copie et le glisser-déposer rendent les annotations enregistrées avec le même moteur que l'éditeur. Tous les formats proposés au destinataire — PNG, texture, URI et liste de fichiers — contiennent les retouches. Une erreur de document, de lecture ou de rendu bloque le partage et produit un retour visible ; il n'y a aucun repli silencieux vers l'original. La copie d'image reste sans URI pour conserver la compatibilité avec le collage dans les navigateurs Chromium.

Les fichiers de partage sont des instantanés PNG privés et distincts dans le cache. Ils restent lisibles après la fin du glisser-déposer et la fermeture de Klypse ; ceux de plus de sept jours sont supprimés lors d'un prochain partage. L'original enregistré sur disque reste intact.

**Validation passée sous Xvfb.** Comparaison des pixels réellement proposés par le presse-papiers et les formats du glisser-déposer, et lecture des fichiers URI : recadrage, rectangle, pixelisation puis flou. Vérification de la conservation de l'original, de la durée de vie des instantanés, du rejet des documents invalides et fichiers absents, et du comportement inchangé des PNG sans retouches, GIF et vidéos. Les deux cibles `annotated_sharing` et `desktop_content` passent, soit trois tests. Cela ne remplace pas une matrice de collage dans chaque application destinataire.

Preuves : [implémentation du partage](/home/roadmvn/Bureau/projet/Klypse/crates/klypse-app/src/desktop/clipboard.rs), [régression des pixels et fichiers transmis](/home/roadmvn/Bureau/projet/Klypse/crates/klypse-app/tests/annotated_sharing.rs).

### 2. Pouvoir reprendre une annotation enregistrée — corrigé et testé

**Constat initial.** La réouverture réinitialisait le compteur d'annotations à `1`. L'ajout suivant pouvait reprendre un identifiant existant et être rejeté sans message visible.

**Correction.** Les identifiants déjà utilisés restent réservés après réouverture, suppression, annulation et rétablissement. Les erreurs d'édition, d'enregistrement, de copie et d'export disposent de messages visibles.

**Validation passée.** Les tests couvrent les identifiants non consécutifs ou personnalisés, la suppression avec annuler/rétablir, puis le parcours enregistrer → fermer le contrôleur → rouvrir → ajouter du texte et une flèche → enregistrer → rouvrir. Les nouveaux objets et les anciens sont conservés, sans modification de l'image originale.

Preuves : [contrôleur de l'éditeur](/home/roadmvn/Bureau/projet/Klypse/crates/klypse-app/src/editor/controller.rs), [régression après réouverture](/home/roadmvn/Bureau/projet/Klypse/crates/klypse-app/tests/editor_controller.rs), [persistance des modifications](/home/roadmvn/Bureau/projet/Klypse/crates/klypse-app/tests/editor_persistence.rs).

### 3. Appliquer les préférences à la capture suivante — corrigé et testé dans l'interface

**Constat initial reproduit dans le bureau isolé.** Après une première capture, changer le dossier de destination puis capturer à nouveau laissait les deux images dans l'ancien dossier. Le moteur conservait les préférences de sa création.

**Correction.** Les paramètres de dossier, copie automatique et notifications sont relus avant chaque nouvelle capture. Les paramètres de destination, notifications, cadence et durée maximale des GIF sont également relus avant un nouvel enregistrement. Un enregistrement actif conserve ses paramètres jusqu'à sa finalisation. Les préférences affichent désormais le dossier effectif, y compris la valeur par défaut.

**Validation passée dans l'interface X11 isolée, sans relancer Klypse.** Après changement du dossier, l'ancienne destination conserve une capture et la nouvelle en reçoit une. La désactivation de la copie automatique conserve le contenu témoin du presse-papiers ; sa réactivation copie bien une image de 1280 × 800 pixels.

Un GIF démarré avec le dossier A, 4 images/s et une limite de quatre secondes conserve ces paramètres après modification des préférences pendant l'enregistrement. Le GIF suivant utilise le dossier B, 8 images/s et une limite d'une seconde. Le réglage des notifications est relu dans le code ; son activation/désactivation ne dispose pas ici d'un essai UI distinct.

Preuves : [rafraîchissement avant les actions](/home/roadmvn/Bureau/projet/Klypse/crates/klypse-app/src/application.rs), [essai dossier et presse-papiers](/tmp/klypse-menu-check/verify-preferences.log), [essai des préférences GIF](/tmp/klypse-menu-check/verify-recording.log). Ces journaux locaux temporaires ne sont pas livrés avec le produit.

### 4. Fiabiliser le flux et la sauvegarde vidéo/GIF — corrigé ; discrétion encore à améliorer

**Constat initial.** Les erreurs du flux vidéo/GIF n'étaient consultées qu'à l'arrêt. Le compteur pouvait continuer malgré une interruption ; la finalisation synchrone pouvait bloquer le traitement GTK.

**Correction.** Les erreurs et fins spontanées du flux (EOS) sont surveillées pendant l'enregistrement. Elles déclenchent la récupération ou une finalisation unique, sans laisser le compteur tourner indéfiniment. L'attente de fin du flux, la génération de miniature et la persistance ne bloquent plus le fil graphique. La durée maximale est rattachée à la session active pour éviter qu'un ancien arrêt automatique affecte l'enregistrement suivant ; les notifications reviennent sur le contexte principal.

**Validation passée.** Les sept tests du contrôleur couvrent notamment l'interruption de source, la fin spontanée du flux, la finalisation lente avec un contexte principal réactif, l'expiration unique de la durée maximale et l'indépendance de la session suivante. Ces résultats ne valident pas encore une révocation du partage sur un vrai bureau Wayland.

L'essai de l'interface confirme aussi qu'un GIF se finalise automatiquement pendant que le sélecteur de capture reste ouvert. La vidéo suivante démarre et s'arrête normalement, sans arrêt obsolète ni erreur indiquant à tort l'absence d'un enregistrement.

**Reste proposé, non réalisé dans ce lot.** Masquer la galerie avant l'enregistrement et proposer une commande d'arrêt discrète. Les boutons vidéo ne masquent pas encore Klypse comme le font les captures ; le début d'un enregistrement peut donc contenir sa fenêtre. Le compte à rebours reste différé, comme demandé. Pause/reprise et son sont des ajouts ultérieurs.

Preuves : [surveillance et finalisation du backend](/home/roadmvn/Bureau/projet/Klypse/crates/klypse-app/src/recording/backend.rs), [contrôleur d'enregistrement](/home/roadmvn/Bureau/projet/Klypse/crates/klypse-app/src/recording/controller.rs), [régressions du contrôleur](/home/roadmvn/Bureau/projet/Klypse/crates/klypse-app/tests/recording_controller.rs).

## Améliorations utiles pour la première version

| Sujet | État actuel constaté | Amélioration proposée |
|---|---|---|
| Export | Un PNG au nom automatique est créé dans le dossier des captures. | « Enregistrer sous » : nom, dossier, PNG par défaut, puis « Exporté » et « Ouvrir le dossier ». |
| Retouche | Création d'objets, annuler/rétablir et suppression de la dernière annotation ; pas de sélection d'un objet existant. | Cliquer pour sélectionner, déplacer, redimensionner, modifier le texte et supprimer cet objet. Réglage de taille du texte et ajustement de l'image à la fenêtre. |
| Vidéo dans la galerie | L'aperçu utilise une image ou une miniature, sans lecteur. | Lecture/pause et barre temporelle ; à défaut, commencer par « Ouvrir dans le lecteur ». |
| Historique | Liste paginée avec miniature, type et date, sans recherche ni titres. | Filtres Images/Vidéos/GIF, dates et renommage ; recherche sur les noms ensuite. |
| Raccourcis | Liste réelle XFCE en lecture seule ; autres bureaux et commandes Flatpak non couverts par cette lecture. | Touches actives, conflit ou non attribué ; modifier et tester depuis un parcours unique, adapté au bureau. |
| Plusieurs écrans | Le sélecteur couvre un moniteur ; capture d'écran entière = bureau X11 complet. | Choix « cet écran / tous les écrans », sélection accessible sur chaque écran, vérification des échelles différentes. |
| Capture différée — reportée | Bouton à cinq secondes, sans progression ni annulation dédiée. | Délais 0/3/5/10 s et annulation ; non urgent selon la demande de l'utilisateur. |
| Retours d'action | Les erreurs d'édition et de partage sont désormais visibles ; les messages de tous les autres parcours restent à revoir. | Harmoniser « Copié », « Exporté », « Fichier introuvable », « Réessayer ». |

Repères dans le code : [éditeur](/home/roadmvn/Bureau/projet/Klypse/crates/klypse-app/src/ui/editor.rs), [galerie](/home/roadmvn/Bureau/projet/Klypse/crates/klypse-app/src/ui/gallery.rs), [raccourcis](/home/roadmvn/Bureau/projet/Klypse/crates/klypse-app/src/ui/shortcuts.rs), [sélecteur de zone](/home/roadmvn/Bureau/projet/Klypse/crates/klypse-app/src/ui/region_overlay.rs).

Les cartes « Zone » et « Fenêtre » ouvrent actuellement le même sélecteur sous X11. Deux possibilités cohérentes : garder trois modes avec une différence explicite de comportement, ou proposer une action principale « Nouvelle capture » avec clic sur fenêtre, glisser pour zone et bouton écran entier. Il faut choisir le parcours le plus compréhensible, sans ajouter un quatrième mode similaire.

## Livraison et validation

Le projet possède déjà des paquets Debian et Flatpak, des workflows CI, des tests et des sommes de contrôle. Le besoin est de vérifier le produit installé et de publier une version correspondant à l'état validé.

**Contrôles locaux passés après les corrections.** `scripts/verify-release.sh` termine avec le code zéro : formatage, traductions, Clippy, tests du workspace, tests média sérialisés, essais GTK/X11 et `cargo deny`. La compilation du binaire en mode release passe également. Le [journal des contrôles](/tmp/klypse-menu-check/verify-fixes-gate.log) est une preuve locale temporaire ; ces résultats ne valent pas validation d'une nouvelle installation Debian/Flatpak ni publication d'une version.

- Tester depuis une installation propre : lancement graphique, français, paramètres, capture, collage et réouverture. Les contrôles de paquets actuels se concentrent surtout sur les métadonnées et l'aide CLI.
- Actualiser la matrice Linux. Son état enregistré date du 14 juillet ; il ne reflète pas les essais X11 récents de cette session. Wayland, plusieurs écrans et collage entre applications doivent conserver des résultats distincts des tests Xvfb.
- Sur Wayland, « Enregistrer une zone » délègue actuellement le choix d'un moniteur ou d'une fenêtre, sans découpage libre du flux. Adapter la promesse affichée ou réaliser ce découpage.
- Le minimum Rust annoncé a été corrigé dans le README : 1.92, conformément aux dépendances GTK retenues.
- Ajouter un accès clair aux notes et téléchargements des versions. Un mécanisme automatique de mise à jour n'est pas indispensable à cette étape.

Preuves : [matrice de compatibilité](/home/roadmvn/Bureau/projet/Klypse/docs/testing/linux-compatibility-matrix.md), [test d'installation Debian](/home/roadmvn/Bureau/projet/Klypse/scripts/test-debian-install.sh), [contrôle Flatpak](/home/roadmvn/Bureau/projet/Klypse/scripts/test-flatpak.sh), [enregistrement Wayland](/home/roadmvn/Bureau/projet/Klypse/crates/klypse-platform/src/portal/recording.rs), [dépendances](/home/roadmvn/Bureau/projet/Klypse/Cargo.toml).

Un essai de capture complète au démarrage sur bureau isolé vide a produit correctement une image noire. Il n'a **pas** reproduit la suspicion de capture accidentelle de la galerie au démarrage. Ce cas reste à couvrir sur de vraies applications et avec la fenêtre active ; ce n'est pas un défaut démontré par cet audit.

## Ordre conseillé

1. **Valider la distribution du lot corrigé** : tester une installation propre et compléter les essais de compatibilité avant publication. Les quatre défauts prioritaires sont corrigés ; les contrôles locaux et essais UI décrits ci-dessus passent.
2. **Usage quotidien** : export avec choix du nom, retouche d'objets, lecture vidéo et confirmations d'action.
3. **Autonomie** : raccourcis configurables, plusieurs écrans, recherche et installation validée.
4. **Ajouts à évaluer ensuite** : refaire la dernière zone, épingler une capture au-dessus des autres fenêtres, extraire le texte localement. Ce sont des propositions de fonctions, pas des défauts démontrés ni des implémentations déjà réalisées.

L'audio vidéo, l'export MP4 et la découpe début/fin méritent un lot séparé si l'enregistrement devient un usage central. La capture, la retouche et le partage restent le parcours prioritaire au vu des besoins exprimés.
