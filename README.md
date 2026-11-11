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
| `.zip` | extrait dans `%LOCALAPPDATA%\Ambyst-App\apps\<app>` |
| autre `.exe` | copié comme app portable |

## Icônes

Ambyst-App cherche l'icône dans le dépôt (`ambyst-icon.png` en priorité, puis `assets/icon.png`, `<Nom>.png`, `<Nom>.ico`, `icons/icon128.png`...). À défaut, il parcourt les fichiers du dépôt et retient l'image qui ressemble le plus à une icône (`icon`, `logo`, `ic_launcher`, nom du dépôt, grande taille). Un dépôt sans icône est revérifié dès qu'il reçoit un nouveau push. Sans icône, une tuile avec l'initiale est affichée.

## Limite GitHub

Sans jeton, l'API GitHub n'autorise que 60 requêtes par heure et par adresse IP, moins que ce qu'il faut pour vérifier toutes les apps. Ambyst-App lit donc la liste des apps, leurs releases et leurs images dans un seul fichier, `catalog.json` (branche `catalog`), téléchargé depuis raw.githubusercontent.com, qui n'est pas soumis à cette limite.

Ce catalogue est régénéré par la GitHub Action `.github/workflows/catalog.yml` toutes les heures et à chaque push sur `main`. Après une release, on peut le mettre à jour tout de suite :

```
gh workflow run catalog.yml -R Les-Doubles-A/Ambyst-App
```

Si le catalogue est inaccessible, Ambyst-App revient à l'API (avec `GITHUB_TOKEN` ou le jeton du GitHub CLI `gh` s'il est connecté). Les dépôts ajoutés à la main passent toujours par l'API.

## Installation

Téléchargez `Ambyst-App-Setup-x.y.z.exe` depuis les [releases](https://github.com/Les-Doubles-A/Ambyst-App/releases). L'installation se fait pour l'utilisateur courant, sans droits administrateur.

## Compiler

```
cargo build --release
iscc installer\ambyst-app.iss
```

Rust (egui/eframe) pour l'application, Inno Setup 6 pour l'installeur. L'icône est générée par `python tools/make_icon.py`.
