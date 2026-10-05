# OG Paper features

The one list of what OG Paper does, where to find each thing, and the
test that proves it works. **When a feature moves or changes, change its
row in the same commit** (that is how "where did Home go?" gets answered).

- **Where**: how a person gets to it, in the words the app uses.
- **Platforms**: W web app · D desktop (Windows, macOS, Linux) · M mobile
  (iOS, Android) · S page server.
- **Tests**: `ui:<name>` is a UI test in `tests/ui/features/<name>.mjs`
  (it doubles as a demo video: `node tests/ui/run.mjs <name> --demo`);
  `rust:<name>` is a Rust test (`cargo test`); `todo` means it has none yet.

Check it all with `scripts/test-all.sh`; `node tests/check-features.mjs`
reports features without tests and tests named here that don't exist.

## Drawing

| ID | Feature | Where | Platforms | Tests |
|---|---|---|---|---|
| DRAW-01 | Draw with the brush (pressure, width, colour, opacity, dashes) | Tool button (bottom right) › Brush; settings in the tool panel | W D M | ui:tools-tour, ui:draw-undo, rust:brush::tests::dabs_follow_spacing_and_repeat, rust:brush::tests::params_round_trip |
| DRAW-02 | Texture brush | Tool button › Texture | W D M | ui:tools-tour, rust:brush::tests::huge_strokes_are_capped |
| DRAW-03 | Highlighter | Tool button › Highlighter | W D M | ui:tools-tour |
| DRAW-04 | Eraser | Tool button › Eraser | W D M | ui:tools-tour, rust:timeline::tests::replays_draws_and_erases |
| DRAW-05 | Shapes: rectangle, ellipse, diamond, triangle, star, hexagon, line, arrow; sloppiness, edges, fills | Tool button › Shapes | W D M | ui:tools-tour, rust:shapes::tests::every_shape_and_fill_makes_ink, rust:shapes::tests::hachure_stays_inside, rust:shapes::tests::same_seed_same_wobble |
| DRAW-06 | Text with bundled and your own fonts | Tool button › Text; tool panel › Add your own font | W D M | ui:tools-tour, rust:font::tests::bundled_fonts_parse_and_draw, rust:font::tests::user_fonts_register_by_family_name, rust:objects::tests::text_scales_with_resize |
| DRAW-07 | Bucket fill (closes small gaps) | Tool button › Bucket | W D M | ui:tools-tour, rust:bucket::tests::floods_inside_and_refuses_outside, rust:bucket::tests::gap_closing_bridges_a_small_gap |
| DRAW-08 | Colour picker (eyedropper) | Tool panel › eyedropper | W D M | todo |
| DRAW-09 | Diagram mode (shapes join with connectors) | Menu › Diagram | W D M | rust:diagram::tests::outlines |

## Editing

| ID | Feature | Where | Platforms | Tests |
|---|---|---|---|---|
| EDIT-01 | Undo and redo | Tool button › Undo / Redo; Ctrl+Z / Ctrl+Shift+Z; two-finger tap | W D M | ui:tools-tour, ui:draw-undo, rust:history::tests::undo_redo_add_and_delete, rust:history::tests::undo_redo_replace |
| EDIT-02 | Select and lasso; move, rotate, scale, duplicate, delete, flip, order | Tool button › Select / Lasso; Selection panel | W D M | ui:tools-tour, rust:objects::tests::rotate_and_scale_ops, rust:edit::tests::point_in_loop |
| EDIT-03 | Copy, cut and paste inside the app | Ctrl+C / X / V; Menu › Paste | W D M | todo |
| EDIT-04 | Paste pictures, text and tables from other apps | Ctrl+V; Menu › Paste | W D M | rust:objects::tests::pasted_tables_parse, rust:objects::tests::tables_and_pictures_make_pieces |
| EDIT-05 | Insert a picture or PDF | Menu › Insert picture / PDF | W D M | rust:images::tests::prepares_and_mips_pictures, rust:pdf::tests::renders_a_page, rust:images::svg_tests::svg_becomes_a_picture |
| EDIT-06 | Crop pictures | Select a picture › Crop | W D M | rust:crop::tests::crop_round_trips_through_the_full_box |
| EDIT-07 | Sticker library (save a selection, place it again) | Menu › Library; Selection panel › Add to library | W D M | rust:library::tests::stickers_round_trip, todo |

## Navigating the endless canvas

| ID | Feature | Where | Platforms | Tests |
|---|---|---|---|---|
| NAV-01 | Endless zoom in and out (exact at any depth) | Mouse wheel, pinch, trackpad | W D M | ui:tools-tour, ui:zoom-home, rust:camera::tests::zoom_in_and_out_1e30_is_lossless, rust:addr::tests::origin_is_exact_at_huge_depth |
| NAV-02 | Pan | Right-drag, two fingers, Tool button › Pan | W D M | rust:camera::tests::point_stays_put_under_cursor_at_depth, todo |
| NAV-03 | Home: fly back to where the canvas starts | Menu › Home; Menu › Bookmarks › Home; ⌂ on the folded bookmarks bar | W D M | ui:tools-tour, ui:zoom-home |
| NAV-04 | Bookmarks: save a view, fly back to it, rename, delete | Menu › Bookmarks | W D M | ui:bookmarks, rust:snapshot::tests::roundtrip_keeps_strokes_timeline_bookmarks_and_view |
| NAV-05 | Bookmarks folded into a bar (Home first, initials, + to save) | Menu › Bookmarks › fold (⌃) | W | ui:bookmarks |
| NAV-06 | Search text and fly to it | Menu › Search text | W D M | rust:search::tests::finds_case_insensitively_with_excerpt, todo |
| NAV-07 | Timeline: scrub and replay the drawing, restore an older state | Menu › Timeline | W D M | ui:tools-tour, rust:timeline::tests::replays_draws_and_erases, rust:timeline::tests::window_leaves_out_older_ink |
| NAV-08 | Portals: a shape or outline that shows a saved view; zoom through it | Tool button › Portal | W D M | rust:portal::tests::a_portal_shows_its_view_at_every_zoom, rust:portal::tests::zooming_down_the_endless_street_goes_round, rust:portal::tests::portals_survive_saving_and_old_apps_see_a_shape |
| NAV-09 | Endless street demo (infinite zoom loop through a portal) | Try mode › Menu › Bookmarks › Endless street | W | rust:portal::tests::zooming_down_the_endless_street_goes_round, todo |

## Pages and files

| ID | Feature | Where | Platforms | Tests |
|---|---|---|---|---|
| FILE-01 | Pages on this device: new (named), open, rename, upload to a workspace | Menu › Pages › This device (select a page; the bar: Open, Rename, Upload, Delete) | W D M | ui:pages-local, ui:upload-local |
| FILE-02 | Autosave; offline copies (save, open) | automatic; Menu › Save copy / Open | W D M | rust:snapshot::tests::roundtrip_keeps_strokes_timeline_bookmarks_and_view, todo |
| FILE-03 | Export PNG, SVG, PDF (all or the selection) | Menu › Export | W D M | rust:export::tests::svg_has_the_items_in_css_px, rust:export::tests::pdf_is_well_formed, rust:export::tests::pictures_embed_and_rasterise |
| FILE-04 | Import a canvas into this one (place it) | Menu › Import canvas | W D M | todo |
| FILE-05 | Merge a copy / save changes (sync by hand) | Menu › Merge copy; Menu › Save changes | W D M | rust:sync::tests::merging_is_order_free_and_idempotent, todo |
| FILE-06 | Sync folder (Dropbox, Drive, Syncthing) | Menu › Sync folder | W D | rust:folder::tests::only_others_changed_files_are_picked_up |

## Sharing: workspaces

| ID | Feature | Where | Platforms | Tests |
|---|---|---|---|---|
| WS-01 | Add a workspace by its link | Menu › Connect to server (an address like paper.example.com, or the server's link); Share live › Join with a server link; "Open in the web app" on the server's page | W D M | ui:connections-tour, ui:workspace, ui:join-workspace-link |
| WS-02 | Sign in to a workspace account (remembered until sign-out or password change) | Menu › Connect to server (your account there); or Pages › select the server › Sign in | W D M | ui:connections-tour, ui:workspace, rust:hub::tests::directory_accounts_decide_what_the_page_list_gives |
| WS-03 | Browse a workspace's folders and open its pages | Menu › Pages: each server is a drive; folders fold open; select a page › Open (or double-click) | W D M | ui:connections-tour, ui:workspace |
| WS-04 | Workspace changes by role: new page / folder, rename, move, delete | Menu › Pages › select a server, folder or page › the bar (New page, New folder, Rename, Move to, Delete) | W D M | ui:workspace, rust:hub::tests::directory_accounts_decide_what_the_page_list_gives |
| WS-05 | Upload a page into a workspace: the open one into a folder, or any page on this device | Menu › Pages › select a server or folder › Upload open page; or select a page on This device › Upload | W D M | ui:workspace, ui:upload-local |
| WS-06 | Sign in on a page to get drawing rights (viewers stay view-only) | Share live (joined with a view link) › Sign in to draw | W D M | rust:hub::tests::page_sign_in_gives_drawing_rights_but_not_to_viewers, todo |

## Sharing: live sessions

| ID | Feature | Where | Platforms | Tests |
|---|---|---|---|---|
| LIVE-01 | Join by pasting a link (a page link joins it; a server link adds the workspace) | Menu › Share live › Paste a link › Join | W D M | ui:connections-tour, ui:share-live, ui:join-workspace-link, rust:net::tests::links_parse_in_every_form |
| LIVE-02 | Host in this browser (no server; invite and reply codes) | Menu › Share live › Host in this browser | W | todo |
| LIVE-03 | Host from the desktop app (a port on this machine) | Menu › Share live › Host | D | todo |
| LIVE-04 | Share through a relay (keeps locked changes for people who come and go) | Menu › Share live › Share via relay | W D M | todo |
| LIVE-05 | See and follow other people | Menu › Share live › Follow / Go to | W D M | ui:connections-tour |
| LIVE-06 | Edits merge without conflicts; rival edits keep the newest (or host / guest wins) | automatic; Share live › rival edits policy | W D M | ui:connections-tour, rust:sync::tests::rival_edits_keep_the_newest_branch_only, rust:sync::tests::the_host_policy_picks_its_own_or_its_guests_edit, rust:wire::tests::every_message_round_trips |
| LIVE-07 | Edit and view links (sealed, signed) | Share live links | W D M S | rust:seal::tests::edit_and_view_keys_seal_open_and_sign |

## Page server (workspace host)

| ID | Feature | Where | Platforms | Tests |
|---|---|---|---|---|
| SRV-01 | Serve pages to the apps (`og-paper --serve-dir DIR`) | command line; Docker on go | S | rust:hub::tests::directory_accounts_decide_what_the_page_list_gives |
| SRV-02 | Home page: how to connect, links, downloads for every client | the server's address in a browser | S | rust:hubpage::tests::the_page_shows_the_view_link_but_never_the_full_one_without_the_key |
| SRV-03 | Console sign-in (first account admin / password), tabs greyed until signed in | server › Sign in | S | rust:hubconsole::tests::roles_decide_what_the_console_allows |
| SRV-04 | Console pages as folders: make, rename, move, delete (tagged), restore | server › Pages | S | rust:hubconsole::tests::roles_decide_what_the_console_allows |
| SRV-05 | Users and roles: admin, subadmin, user, viewer; reset and change passwords | server › Users; server › Password | S | rust:hubconsole::tests::roles_decide_what_the_console_allows |
| SRV-06 | Automation API (JSON over WebSocket at /api; JS and Python clients) | `ws://server/api` | S | todo |

## Layout and settings

| ID | Feature | Where | Platforms | Tests |
|---|---|---|---|---|
| UI-01 | Menu (gear, top right) grouped in sections | gear button | W D M | ui:menu |
| UI-02 | Tool fan (undo, redo, tools) | Tool button (bottom right) | W D M | ui:draw-undo |
| UI-03 | Quick toolbar with numbered slots and a hold slot per toolbar | Menu › UI › Quick toolbar | W D M | ui:tools-tour, rust:hotbar::tests::slots_round_trip, rust:hotbar::tests::names_and_many_toolbars_survive |
| UI-04 | Inventory: drag tools between slots; drag onto the trash to delete | Quick toolbar › bag | W D M | rust:hotbar::tests::inventory_keeps_a_free_row, todo |
| UI-05 | Radial toolbar; page shaded only under an open fan | Menu › UI › Radial toolbar | W D M | rust:layout::tests::fans_open_toward_the_middle, todo |
| UI-06 | Edit layout (move the buttons) | Menu › UI › Edit layout | W D M | rust:layout::tests::layouts_round_trip, todo |
| UI-07 | UI size | Menu › UI › UI size | W D M | todo |
| UI-08 | Hide helper text | Menu › UI › Hints | W D M | todo |
| UI-09 | Dark mode and grid | Menu › UI | W D M | todo |
| UI-10 | Hotkeys (view and change) | Menu › Hotkeys | W D | rust:hotkeys::tests::defaults_have_no_clashes, rust:hotkeys::tests::keymap_changes_round_trip_and_keys_move |
| UI-11 | Full screen | Menu › Full screen | W | todo |
| UI-12 | Movable cards (drag the title; double-tap to reset) | any card's title bar | W | ui:bookmarks |
| UI-13 | Phone keyboard for the app's text boxes | tap any text box on a phone | W | ui:share-live |
| UI-14 | Paste, copy and cut go to the focused text box | Ctrl+V in a text box | W D | ui:share-live |
| UI-15 | Try mode tour | omegagiven.github.io/OG-paper/try/ › Tour | W | todo |

## Plugins

| ID | Feature | Where | Platforms | Tests |
|---|---|---|---|---|
| PLUG-01 | WASM plugins | Menu › Plugins | W D | rust:plugin::tests::the_example_plugin_loads_and_answers |
| PLUG-02 | .ogpack packs (toolbars, tools, stickers) | Menu › Plugins › Install pack | W D | rust:pack::tests::packs_read_toolbars_tools_and_stickers |
| PLUG-03 | Scripts (automation inside the app) | Menu › Plugins; `window.ogPaper.run` | W D | rust:script::tests::colors_parse, todo |

## Engine (no UI)

| ID | Feature | Where | Platforms | Tests |
|---|---|---|---|---|
| ENG-01 | Millions of strokes stay fast (visible set bounded) | — | W D M | rust:tests::million_strokes_visible_set_stays_bounded, rust:visible::tests::query_matches_brute_force_under_random_pan_and_zoom |
| ENG-02 | File format round trips (deep cells, tombstones, sync log) | — | W D M | rust:tests::roundtrip_with_deep_cells_and_tombstones, rust:tests::sync_log_and_canvas_id_round_trip |
| ENG-03 | GPU shaders validate | — | W D M | rust:render::tests::shader_validates |
