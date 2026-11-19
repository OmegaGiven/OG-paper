# Store material (task #974, 2026-11-19)

Copies of what OG Paper's two store listings use; sources and scripts stay in `packaging/`.

| Store | Id | State |
|---|---|---|
| App Store Connect | `com.omegagiven.ogpaper`, app 6818948230 | iOS 1.0/1.1 live; iOS 1.2(13) and Mac 1.2(14) WAITING_FOR_REVIEW (manual release) |
| Google Play | `app.ogpaper`, app 4974630082929640996 | Console draft; 1.2 (1002) on closed test (alpha), changes in review |

Both records, listings, ratings and privacy answers were already done by tasks #844/#847/#884/#876/#905;
this task verified them through the ASC API and added nothing to what is submitted.

- `play/`: `icon-512.png`, `feature-graphic.png` (1024x500), `phone/` (1080x2160), `tab7/`, `tab10/`, `listing/` (title, short, full description). Real-app screenshots with demo canvases (`packaging/android/play/README.md`).
- `apple/`: `iphone-69/`, `ipad-13/` screenshots, `description*.txt`, `keywords.txt`, `promo-text.txt`, `icon-1024.png`.
- Short promo text (Play short description): "An endless canvas: sketch, write and diagram, then zoom in forever."

## Verified in ASC (read only)
Category Graphics & Design, age rating 4+, price free, subtitle "The endless canvas", keywords, support
https://omegagiven.github.io/OG-paper/support/, marketing https://omegagiven.github.io/OG-paper/, privacy
https://omegagiven.github.io/OG-paper/privacy/. App Privacy answers are not in the API; iOS 1.0/1.1 are live, so they were answered.

## Still missing (owner)
1. Play: ogstudio.org/paper/privacy/ is 404; deploy omegagiven-com `public/paper` from web-dist 80ca10a.
2. Play: add 12+ tester Gmails (Google Group ogpaper-testers, `docs/LAUNCH-KIT.md`), then Publishing overview > Send changes for review; 14 days later Apply for production.
3. Play: EU trader status banner unanswered.
4. Apple: press nothing; 1.2 reviews are pending. Mac 1.2 has no What's New (first Mac version).
5. Play listing uses omegagiven.com-era contact (omegagiven9@gmail.com); ogstudio.org is the official site now, switch when its privacy page is live.
