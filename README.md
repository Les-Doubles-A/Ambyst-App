# Ambyst-App

<img src="assets/icon-256.png" width="96" alt="Ambyst-App">

Lanceur d'applications minimaliste pour Windows. Il répertorie automatiquement toutes les apps publiées par [Les-Doubles-A](https://github.com/Les-Doubles-A), les installe depuis leur dernière release GitHub et signale les mises à jour.

## Utilisation

- La liste se charge toute seule : chaque dépôt Les-Doubles-A qui a une release apparaît avec son icône.
- **Installer** télécharge et installe la dernière release.
- **Installé** indique que l'app est présente et à jour ; **Mettre à jour** apparaît quand une nouvelle release sort.
- **Ouvrir** lance l'app, **Désinstaller** utilise son désinstalleur Windows.
- Onglets **Toutes / Installées / Mises à jour**, recherche, et **Tout mettre à jour** en haut.
- Pour suivre un dépôt d'un autre compte, collez `propriétaire/dépôt` dans la recherche puis **Ajouter**.

Les apps déjà installées sont reconnues grâce à la liste « Applications installées » de Windows.
Les apps Android (APK seul) sont listées avec un lien vers leur release.

## Fichiers pris en charge

| Fichier | Action |
| --- | --- |
| `*setup*.exe`, `*install*.exe` | lance l'installeur |
| `.msi` | installe via `msiexec` |
| `.zip` | extrait dans `%LOCALAPPDATA%\Ambyst-Apppps\<app>` |
| autre `.exe` | copié comme app portable |

## Icônes

Ambyst-App cherche l'icône dans le dépôt (`ambyst-icon.png` en priorité, puis `assets/icon.png`, `<Nom>.png`, `<Nom>.ico`, `icons/icon128.png`...). Sans icône, une tuile avec l'initiale est affichée.

## Limite GitHub

Les réponses de l'API sont mises en cache (ETag) : après le premier lancement, les vérifications ne consomment presque plus de quota. Si `GITHUB_TOKEN` est défini, ou si le GitHub CLI (`gh`) est connecté, son jeton est utilisé.

## Installation

Téléchargez `Ambyst-App-Setup-x.y.z.exe` depuis les [releases](https://github.com/Les-Doubles-A/Ambyst-App/releases). L'installation se fait pour l'utilisateur courant, sans droits administrateur.

## Compiler

```
cargo build --release
iscc installer\ambyst-app.iss
```

Rust (egui/eframe) pour l'application, Inno Setup 6 pour l'installeur. L'icône est générée par `python tools/make_icon.py`.
