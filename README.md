# Rustify
Download Spotify tracks (with a premium account).

This program uses [librespot](https://github.com/librespot-org/librespot), and as such, requires a Spotify Premium account to use. It supports downloading single tracks and episodes, as well as entire playlists, albums, and shows.

## Requirements
Docker installed

## Usage
Simply run (Lunix/Unix):

```sh
docker run --rm -i -v "$(pwd):/data" fran1240/oggify:latest
```

Or Windows (Powershell terminal):

```sh
docker run --rm -i -v "${pwd}:/data" fran1240/oggify:latest
```
The program will be ready to accept URI/urls via stdin on each enter. After entering the links, type "done" to start the download

### Using a File
For example, create a file named links.txt with the Spotify URLs or URIs you want to download:
```
spotify:track:4uLU6hMCjMI75M1A2tKUQC
spotify:album:1ATL5GLyefJaxhQzSPVrLX
spotify:playlist:37i9dQZF1DXcBWIGoYBM5M
```
Then run the following command:
```sh
docker run --rm -i -v "$(pwd)":/data fran1240/oggify:latest < links.txt
```

### Login
The first time you run the program, it will provide you with a URL to copy and paste into your browser. Follow these steps:

1. Open the provided URL in your browser.
2. Click on "Accept" to grant the necessary permissions.
3. Spotify will redirect you to another URL. This URL will likely show an error message, but that's expected.
4. Copy the entire URL from your browser's address bar.
5. Paste the copied URL into the terminal where the program is running and press Enter.

The program will store your credentials in the cache for future use, so you won't need to repeat this process every time.

### Folder structure
All URIs/URLs will be organized into specific folders based on their type:

- **Tracks**: All individual tracks will be stored in the `tracks` folder.
- **Playlists**: Playlists will be stored inside the `playlists/<name-of-playlist>` folder.
- **Albums**: Albums will be stored inside the `albums/<name-of-album>` folder.
- **Others**: Other types of content will be organized similarly based on their category.
