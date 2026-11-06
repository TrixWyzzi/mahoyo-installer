# Installateur Mahoyo

Installateur Windows natif écrit en Rust pour **Mahoutsukai no Yoru, édition PC japonaise de 2012**. Il prépare une installation neuve avec la traduction française, les voix japonaises et le correctif officiel de compatibilité Windows utilisés sur le PC de référence.

La traduction française et les patchs de traduction proviennent de **[MoonTrad](https://github.com/IDerr/mahoyo-french)**.

Le dépôt Git contient notre installateur et les empreintes de vérification. L'image du jeu de base est distribuée en plusieurs fichiers dans les Releases. Les patchs français et les voix sont récupérés depuis leurs sources ; aucune sauvegarde personnelle n'est distribuée.

## Installer sur un autre PC

1. Télécharger l'archive `Mahoyo-Installateur.zip` dans la [version v0.2.4](https://github.com/TrixWyzzi/mahoyo-installer/releases/tag/v0.2.4), puis l'extraire dans un dossier où vous pouvez écrire.
2. Ouvrir `Installer-Mahoyo.exe`, accepter la demande Windows de droits administrateur, puis cliquer sur **Installer** pour utiliser le chemin affiché. Pour changer d’emplacement, cliquer sur **Modifier…** et sélectionner un disque ou un dossier de jeux. Le launcher propose automatiquement un nouveau sous-dossier, numéroté si le nom existe déjà.
3. Laisser l'installateur télécharger le jeu et les patchs. Il réunit les fichiers de l'image disque et vérifie les empreintes avant de continuer. Une ISO locale de l'édition PC 2012 peut également être utilisée.
4. À la fin, cliquer sur **Lancer le jeu** ou utiliser le raccourci **Français et Voix** du Bureau.

Une connexion Internet est nécessaire au premier passage pour environ **11 Go** au total, ou **3,7 Go** de patchs si l'ISO est déjà disponible. Les nouveaux téléchargements sont stockés dans `.mahoyo-cache/donnees`, à côté du dossier choisi pour le jeu. Les données déjà valides fournies avec l’installateur sont réutilisées. Prévoir **30 Go libres** pour un premier passage avec téléchargement et installation sur le même disque. Les morceaux de l'ISO sont supprimés après assemblage vérifié ; le jeu installé occupe environ 11 Go. Une installation existante et ses sauvegardes ne sont jamais remplacées : choisir un nouveau dossier.

Le téléchargement est public. Aucun identifiant GitHub n'est demandé par l'installateur.

## Configuration

- Windows 10 ou 11, 64 bits. Le launcher utilise les contrôles natifs de Windows et Windows PowerShell pour le disque virtuel et le raccourci.
- Pour jouer : langue des programmes non Unicode **Japonais (Japon)**, option **Bêta : utiliser Unicode UTF-8** désactivée, puis redémarrage de Windows après un changement. Si ces paramètres sont incompatibles au moment de cliquer sur Installer, une confirmation propose le réglage en japonais et la désactivation d’UTF-8 bêta. Ce réglage système concerne tous les programmes non Unicode. Redémarrer Windows manuellement avant de jouer.
- L'image disque attendue mesure 7 310 540 800 octets ; SHA-256 : `A7A7483DE792E15C4E034B330C94CD4532432C0A4C57A86A4E6E3652FCE909BE`.
- La version Steam n'est pas prise en charge par cet installateur.

## Sources

- **MoonTrad** fournit les patchs français 1, 2, 3 et 8. L’installateur utilise la [version du projet correspondant à ces patchs](https://github.com/IDerr/mahoyo-french/tree/44b5691a9ad1f5b8858240028c23a605cd56cc0a).
- [Données des voix japonaises](https://drive.google.com/drive/folders/1cTuGCekNnG6_dNz4vPdeqLTRFtyTJs77) : patchs 4 à 7, [référence VNDB](https://vndb.org/r109993).
- [Correctif officiel TYPE-MOON](https://www.typemoon.com/users/download/?pc=WOHN) : `forWin8x64.zip`.

Les données sont téléchargées depuis ces fournisseurs et contrôlées par taille et SHA-256 avant utilisation. Leur disponibilité et leurs quotas dépendent de ces fournisseurs. Après une interruption, les fichiers déjà entièrement téléchargés et vérifiés sont réutilisés ; un téléchargement interrompu reprend au dernier octet conservé si le serveur accepte la reprise. Sinon, seul le fichier en cours recommence. Une page de quota ou de confirmation est refusée avec un message de réessai. Le dépôt ne donne aucun droit de redistribution sur le jeu ou les contenus tiers.

## Compiler et vérifier

Installer Rust stable pour Windows x64 avec les outils C++ de Visual Studio et le SDK Windows, puis exécuter depuis la racine du dépôt dans PowerShell :

```powershell
& .\sources\build.ps1
```

Le script lance les tests Cargo en mode release, compile le launcher et copie le résultat dans `Installer-Mahoyo.exe`. Le manifeste SHA-256, le fond et l’icône sont intégrés lors de la compilation. Un dossier `donnees` voisin peut fournir tout ou partie du jeu pour une installation hors ligne.

Avec la chaîne GNU x64, placer MinGW-w64 (GCC, windres et ar) dans le PATH. Avec w64devkit, sélectionner la chaîne Rust GNU et son environnement de liaison avant d’exécuter le même script :

```powershell
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-gnu'
$env:RUSTFLAGS = '-C link-self-contained=yes'
```

## Vérification

Les tests couvrent la reprise HTTP, les serveurs qui ignorent la reprise, les empreintes, les chemins occupés et le nettoyage après un échec. Pour tester toute l’installation dans un dossier temporaire, définir `MAHOYO_TEST_PACKAGE` sur le dossier du paquet hors ligne et `MAHOYO_TEST_SCRATCH` sur un dossier de travail disposant d’au moins 12 Go libres, puis lancer :

```powershell
cargo test --release --locked offline_install -- --ignored
```

Ce test monte le disque, installe le jeu dans le dossier temporaire et vérifie les huit patchs et les six correctifs avant le nettoyage.

## Vérifier le téléchargement

Dans cette version, les morceaux de l’ISO et les patchs manquants sont transférés deux par deux. Les transferts interrompus restent récupérables et chaque fichier doit correspondre à son empreinte avant utilisation.

La commande PowerShell `& .\sources\package.ps1 -OutputDirectory .\dist -SigningCertificateThumbprint EMPREINTE_DU_CERTIFICAT` compile le projet, lance ses tests, signe le launcher avec le certificat de signature de code installé dans le magasin Windows et contrôle l’exécutable puis le ZIP avec Microsoft Defender avant de produire l’archive et `SHA256SUMS.txt`. La signature doit être approuvée par Windows et horodatée. Pour préparer une préversion non signée, utiliser explicitement `-AllowUnsignedPreview` à la place du certificat. Comparer les empreintes avec `Get-FileHash .\Mahoyo-Installateur.zip` et, après extraction, `Get-FileHash .\Installer-Mahoyo.exe`. Ces contrôles identifient les fichiers distribués. En cas d’alerte, conserver le fichier en quarantaine et joindre au signalement son empreinte, le nom du moteur et le verdict exact.


## Avertissement SmartScreen

La v0.2.4 publiée est non signée. Elle peut afficher « Windows protected your PC » lors du lancement d’un téléchargement. Un scan antivirus réussi et des empreintes correctes ne lèvent pas cet avertissement de réputation.

La signature de production exige un certificat Authenticode émis après vérification de l’identité de l’éditeur. Une nouvelle application signée peut également afficher une alerte pendant la construction de sa réputation. Voir la [documentation Microsoft sur SmartScreen](https://learn.microsoft.com/windows/apps/package-and-deploy/smartscreen-reputation).

La préparation des archives exige désormais un certificat de signature ou le choix explicite d’une préversion non signée. Vérifier la signature d’un exécutable extrait avec `Get-AuthenticodeSignature .\Installer-Mahoyo.exe`. La disparition de SmartScreen doit être vérifiée sur le téléchargement publié.
