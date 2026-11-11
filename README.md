# Ambyst-App

<img src="assets/icon-256.png" width="96" alt="Ambyst-App">

Lanceur d'applications minimaliste pour Windows. Ajoutez un dépôt GitHub, Ambyst-App installe sa dernière release et vous prévient quand une mise à jour est disponible.

## Utilisation

1. Collez `propriétaire/dépôt` ou un lien `https://github.com/...` puis cliquez sur **Ajouter**.
2. Cliquez sur **Installer**.
3. Quand une nouvelle release sort, le bouton devient **Mettre à jour** (ou **Tout mettre à jour** en haut).

## Fichiers pris en charge

Ambyst-App choisit automatiquement le fichier Windows de la dernière release :

| Fichier | Action |
| --- | --- |
| `*setup*.exe`, `*install*.exe` | lance l'installeur |
| `.msi` | installe via `msiexec` |
| `.zip` | extrait dans `%LOCALAPPDATA%\Ambyst-App\apps\<app>` |
| autre `.exe` | copié comme app portable |

La liste est enregistrée dans `%LOCALAPPDATA%\Ambyst-App\apps.json`.
Pour dépasser la limite de l'API GitHub (60 requêtes/heure), définissez la variable d'environnement `GITHUB_TOKEN`.

## Installation

Téléchargez `Ambyst-App-Setup-x.y.z.exe` depuis les [releases](https://github.com/Les-Doubles-A/Ambyst-App/releases). L'installation se fait pour l'utilisateur courant, sans droits administrateur.

## Compiler

```
cargo build --release
iscc installer\ambyst-app.iss
```

Rust (egui/eframe) pour l'application, Inno Setup 6 pour l'installeur. L'icône est générée par `python tools/make_icon.py`.
