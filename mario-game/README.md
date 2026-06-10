# Pocket Plumber 🍄

A Mario-style platformer that runs in the browser, optimized for iPhone.
It's a single self-contained HTML file — no build step, no dependencies.

## Play it on iPhone

1. Host `index.html` anywhere (or open it from Files). The quickest way from this repo:

   ```sh
   cd mario-game
   python3 -m http.server 8080
   # then on your iPhone (same Wi-Fi): http://<your-computer-ip>:8080
   ```

2. Open the URL in Safari and **rotate to landscape**.
3. For a fullscreen, app-like experience: tap **Share → Add to Home Screen**,
   then launch it from the home screen icon.

## Controls

| iPhone | Desktop |
| ------ | ------- |
| ◀ / ▶ on-screen buttons | Arrow keys or A/D |
| **A** button to jump (hold for higher jumps) | Space / ↑ / W |

## Gameplay

- Collect coins and hit **?** blocks for points.
- Hit a **!** block to grow big — big players can smash bricks and survive one hit.
- Stomp enemies; don't fall into pits; beat the clock.
- Reach the flag at the end of the course to win.
