# Launch kit: recruiting 12 testers

Drafts for the owner to post. Nothing here has been posted and no accounts were created.
Goal: 12+ people opted in to the closed test and **keeping the app installed 14 days**,
then Dashboard → "Apply for production".

## Links

| What | URL |
|---|---|
| Join the group | https://groups.google.com/g/ogpaper-testers |
| Opt in (web) | https://play.google.com/apps/testing/app.ogpaper |
| Play Store | https://play.google.com/store/apps/details?id=app.ogpaper |
| Landing (explains the steps) | https://github.com/OmegaGiven/OG-paper#become-a-tester-android |

The opt-in and Play URLs follow Google's fixed formats for package `app.ogpaper`. The group
link only works after the group exists (next section). Use one link per post where you can.

## Before you post (needs you: 3 minutes)

The group could not be created from the forge's browser: Google Groups showed a reCAPTCHA on "Create group".

1. https://groups.google.com → Create group. Name `OG Paper testers`, email `ogpaper-testers`
   (if taken, pick another and replace it in README.md, docs/SITE-TESTER-SECTION.md and here).
2. Privacy: who can search = **Anyone on the web**, who can join = **Anyone can join**;
   conversations and members stay **Group members**. Create group.
3. Play Console → OG Paper → Closed testing → Alpha → Manage track → Testers tab →
   **Google Groups** → add `ogpaper-testers@googlegroups.com` → Save. Saving sends the change
   for review. The existing "Fulfillment testers" email list belongs to another app; uncheck it here.
4. Same tab, "How testers join your test" → copy **Join on the web**. It should equal the opt-in
   URL above; if not, replace it everywhere.
5. Feedback URL or email on that tab is already filled.

## Images and clips (real app screenshots)

All in `packaging/android/play/phone/` (real screenshots from the app).

1. `01-street.png`: street drawing. Hero image for X, Bluesky, Mastodon, Facebook.
2. `04-deep.png`: deep zoom. Caption: "Same canvas. 10^45 times deeper." Reddit and video.
3. `05-tools.png`: tools. For note-taking audiences.
4. Clip (10 s screen recording on a phone): start zoomed out on the street scene, pinch-zoom into a
   tiny detail until text appears, pull back out. Loop it. Use as the video on X, Bluesky, Mastodon
   and as the YouTube Short.

## Short posts

### X (under 280)
> I made an infinite canvas you can zoom into forever. Write inside the dot of an "i", zoom out until a notebook is a speck. Open file format, free.
> Android needs 12 testers for 14 days: https://github.com/OmegaGiven/OG-paper#become-a-tester-android
> [clip]

### Bluesky (under 300)
> OG Paper: an open-source endless canvas, infinite pan and zoom. Your files are SQLite you own, not a locked-in format.
> Looking for 12 Android testers for Google's 14-day closed test. Two minutes:
> https://github.com/OmegaGiven/OG-paper#become-a-tester-android [clip]

### Mastodon (500, with hashtags)
> OG Paper is an open-source, truly endless canvas: pan and zoom without limit, draw in the dot of an "i", then zoom out until the whole notebook is a speck. Files are SQLite with a published spec, so they outlive the app. Windows, macOS, Linux, Android, iOS and the web.
>
> Google needs 12 testers for 14 days before the Android app can go public. On Android? Join the group, opt in, install, keep it two weeks: https://github.com/OmegaGiven/OG-paper#become-a-tester-android
>
> #OpenSource #Android #NoteTaking #FOSS [clip]

### Facebook (your timeline, or groups that allow it)
> I've been building OG Paper, an endless drawing and note canvas. You can zoom in forever, so a whole page can live inside a tiny dot. Free, open source, and your files stay yours.
>
> It's on Google Play in a closed test and I need 12 Android testers for 14 days to get it released. Two minutes: join the group, opt in, install, keep it installed. Link in the first comment: https://github.com/OmegaGiven/OG-paper#become-a-tester-android
> Thank you, it really helps.

### Reddit

Rules change: read each sidebar first, post once per sub, spread posts over several days,
say you are the developer, never ask for upvotes, reply to every comment.

| Subreddit | Fit | What to post |
|---|---|---|
| r/AndroidClosedTesting | Made for tester swaps | "OG Paper (infinite canvas notes, free): 12 testers wanted, I'll test yours back." Then do. |
| r/androiddev | Developer peers | Use its showcase/tester thread if it has one; a standalone recruiting post may be against the rules. Ask for feedback on pen input and performance. |
| r/AndroidApps | App showcase | The clip, plus "developer, free app, in testing". Check the flair/format rules. |
| r/NoteTaking | Note-taking audience | Lead with the zoom idea and open file format; the tester ask goes last. |
| r/PKMS | Knowledge tools | Angle: open SQLite format, export, no lock-in. Only if self-promotion is allowed. |
| r/opensource, r/SideProject | Open and indie | Show-and-tell tone: why endless canvas, what you learned building it. |

Skip r/Android and any sub that bans app promotion.

Post body (change the title per table):

> **Title:** I built an open-source infinite canvas you can zoom into forever (Android testers wanted)
>
> I'm the developer. OG Paper is a pan-and-zoom canvas with no depth limit: write a sentence inside the dot of an "i", zoom out until a notebook is a speck. One SQLite `.ogp` file per canvas with an open spec, plus SVG/PDF/PNG export. Web, desktop and Android.
>
> Google needs 12 testers to keep the app installed for 14 days before release. On Android: (1) join https://groups.google.com/g/ogpaper-testers (2) opt in at https://play.google.com/apps/testing/app.ogpaper (3) install from Play. Feedback in comments or by email. Happy to test your app in return.

## YouTube Short (under 40 s, vertical)

Hook first, no intro.

| Time | Shot | Line / on-screen text |
|---|---|---|
| 0-3 | Phone zoomed into a dot, a word appears | "This word lives inside the dot of an i." |
| 3-12 | One continuous pinch-out until the drawing is a speck | "Zoom out. And out. A whole notebook is a speck." |
| 12-22 | Quick cuts: pen strokes, a diagram, the tools | "Draw, write, diagram. No page limit." |
| 22-30 | Files screen, export menu | "Your files are open SQLite. They outlive the app." |
| 30-38 | End card with the opt-in URL | "Free on Android. Help me test it for 14 days." |

Title: `I made a notebook you can zoom into forever`. Description: the three links.

## 2-3 minute video script

Title: `OG Paper: an endless canvas that never locks you in (Android testers wanted)`

| Time | Shot | Voiceover |
|---|---|---|
| 0:00-0:15 | Cold open: zoom from the street scene into a dot until text appears | "That sentence is inside the dot of an i, on the same canvas as the street you just saw." |
| 0:15-0:35 | Title card with the OG tile icon | "I built OG Paper because infinite-canvas apps keep your notes in formats only they can open." |
| 0:35-1:05 | Phone: pan and zoom demo (street, then deep zoom) | "Pan and zoom have no limit. Only what is visible gets drawn, so it stays fast." |
| 1:05-1:35 | Pen, shapes, diagrams, text | "Ink, shapes, diagrams and text, all on one plane." |
| 1:35-2:00 | Desktop: open the `.ogp` in a SQLite viewer; export menu | "A file is plain SQLite with a public spec. You can read it with a few lines of Python." |
| 2:00-2:15 | Web app and desktop beside the phone | "Android, iOS, desktop and browser. Pick a canvas up anywhere." |
| 2:15-2:45 | Phone: group page, opt-in page, Play install button, URLs on screen | "Google needs twelve testers to keep the app installed for fourteen days. Join the group, opt in, install." |
| 2:45-3:00 | End card | "Links in the description. Tell me what breaks. Thank you." |

Shot list to record once: (1) 30 s pinch-zoom from far out to a tiny detail and back,
(2) 15 s of pen drawing, (3) the three tester steps on a real phone, (4) desktop file open and export,
(5) a still of each screenshot in `packaging/android/play/phone/`. Record 1080x1920 for the Short,
1920x1080 for the long cut, 60 fps. End card: the OG tile (dark `#16181d`, off-white text).

## After posting

- Play Console → Testers tab shows how many opted in; check daily.
- Thank each tester and ask them to keep the app installed. If opt-ins fall below 12 the 14 days restart.
- Once 12 stayed opted in for 14 days: Dashboard → "Apply for production".
