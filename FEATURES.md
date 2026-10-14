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
| DRAW-09 | Diagram mode: lines and arrows join shapes, texts and pictures, onto green connection points (fixed) or the outline (floating, faces the other end); every edit of joined things (moving several at once, resizing, turning, restyling, editing a text) re-routes them in the same undo step, on screen or not | Menu › Diagram; Shapes › Line or Arrow | W D M S | ui:diagram-links, rust:diagram::tests::outlines, rust:diagram::tests::lines_on_connection_points_stay_on_them, rust:diagram::tests::moving_both_ends_objects_moves_the_whole_line, rust:diagram::tests::floating_ends_turn_to_face_each_other, rust:diagram::tests::a_text_growing_keeps_its_lines, rust:diagram::tests::lines_merely_near_a_small_thing_do_not_follow_it, rust:diagram::tests::far_from_the_view_lines_still_follow, rust:diagram::tests::drawn_ends_take_connection_points_or_float, rust:diagram::tests::ports_and_links |
| DRAW-10 | Layers: new layers draw above the rest; pick the one to draw on, hide (not drawn or picked), lock (not picked, erased or drawn on), rename, move up and down, delete (undo brings it back), move the selection onto one. Which layer a thing is on travels with it (files, sync, undo); names and hidden / locked states are kept per canvas on each device | Menu › Layers | W D M | ui:layers, rust:layers::tests::layers_keep_order_hide_lock_and_move, rust:layers::tests::layer_states_round_trip |
| DRAW-11 | Layer looks: each layer's opacity, blend mode (normal, multiply, screen, add, darken, lighten) and clipping mask (shows only where the layer under it has something); drawn on the GPU per layer and kept in SVG/PNG (and PDF, without clipping) exports | Menu › Layers › Opacity / Blend / Clip | W D M | ui:layer-looks, rust:layers::tests::layer_looks_plan_and_export |
| DRAW-12 | Pen paths: tap for corners, press-drag for smooth curves, tap the first point to close (Enter, Escape or the last point finishes it open); styled, filled, moved and resized like any shape; AIs add them with kind path | Shapes › Pen (curves) | W D M | ui:pen-booleans, rust:pen::tests::pen_draws_corners_and_curves_and_closes, rust:pen::tests::paths_from_points_open_and_closed |
| DRAW-13 | Gradients: linear (any angle) and radial shape fills from the fill color to an end color, kept in saves, sync and exports; AIs set them with gradient | Shapes › Fill › Gradient / Radial gradient, Color › Fill end, Angle | W D M | ui:gradients, rust:shapes::tests::gradients_fill_in_bands_from_one_color_to_the_other, rust:snapshot::tests::gradient_shapes_keep_their_end_color_and_angle, rust:script::tests::gradients_combine_and_groups_by_id |
| DRAW-14 | Frames (artboards): named white pages under what is drawn, their names over them; renamed when selected; Menu › Frames flies to each; Export of a selected frame exports everything inside it, cut to it; AIs see them as kind frame | Shapes › Frame; Menu › Frames | W D M | ui:frames, rust:frames::tests::frames_are_named_saved_and_exported_whole |
| DRAW-15 | Text styling: letter spacing and line height; text on a path (a line, arrow, pen path, shape outline or ink stroke) and back off it; AIs set letter_spacing, line_height and path | Text panel › Spacing; Selection panel › Text on path / Off path | W D M | ui:text-style, rust:textpath::tests::text_runs_along_a_path_and_back, rust:textpath::tests::letter_and_line_spacing_grow_the_box |

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
| EDIT-08 | Delete button on a selection (a ✕ off its top right corner, clear of the handles) | Select › ✕ | W D M | ui:delete-button |
| EDIT-09 | Details tab for a selection: what it is, made when and by whom, how often it changed, facts by kind | Select › Details | W D M | ui:details-tab, rust:details::tests::a_new_shape_was_made_by_you_and_never_changed |
| EDIT-10 | Drag-select favours what is picked out (a bucket fill only when wholly inside); copying texts gives other apps their plain text | Select › drag; Ctrl+C | W D M | ui:select-texts |
| EDIT-11 | Arrange: line the selection up (left, centres, right, tops, middles, bottoms), spread it out evenly across or down; a dragged selection snaps its edges and centre to what is around, with a pink guide (Alt: no snapping) | Selection panel › ⇤ ⇹ ⇥ ⤒ ⇳ ⤓ ⇿ ⇕; drag | W D M | ui:arrange, rust:arrange::tests::align_lines_things_up, rust:arrange::tests::distribute_makes_gaps_equal, rust:arrange::tests::snapping_pulls_edges_onto_targets |
| EDIT-12 | Groups: several things picked, moved, resized and styled as one; a tap on any member picks the group; Ungroup splits it; saved and shared live | Selection panel › Group / Ungroup; Ctrl+G (with things selected) / Ctrl+Shift+G | W D M | ui:groups, rust:groups::tests::groups_pick_move_save_and_ungroup_as_one, rust:groups::tests::a_group_lines_up_as_one |
| EDIT-13 | Shape booleans: union, subtract (upper from lowest), intersect, exclude of the selected shapes into one pen path with holes, one undo step | Selection panel › Union / Subtract / Intersect / Exclude | W D M | ui:pen-booleans, rust:booleans::tests::booleans_of_two_squares, rust:booleans::tests::subtracting_on_the_page_makes_one_path |
| EDIT-14 | Numbers: the selection's X, Y, W, H (home-view points, as AIs use) and turn in the selection panel; type or drag one to move, resize or turn it, one undo step | Selection panel › X Y W H ° | W D M | ui:measure, rust:measure::tests::numbers_place_size_and_turn_the_selection |
| EDIT-15 | Picture colors: brightness, contrast and saturation (drawn, saved, exported to SVG/PNG); Remove background makes a picture's plain background see-through in a new copy; AIs set them and remove_background | Selection panel › Brightness / Contrast / Saturation, Remove background | W D M | ui:image-adjust, rust:imagefx::tests::a_plain_background_goes_and_the_subject_stays, rust:export::tests::pictures_embed_and_rasterise |
| EDIT-16 | Components: make a selection a component's main; duplicates are copies that are remade whenever the main changes (same undo step), keeping their own place and size; Detach, Go to main; saved and synced; AIs see component and main | Selection panel › Make component / Go to main / Detach | W D M | ui:components, rust:components::tests::copies_follow_their_main |

## Navigating the endless canvas

| ID | Feature | Where | Platforms | Tests |
|---|---|---|---|---|
| NAV-01 | Endless zoom in and out (exact at any depth) | Mouse wheel, pinch, trackpad | W D M | ui:tools-tour, ui:zoom-home, rust:camera::tests::zoom_in_and_out_1e30_is_lossless, rust:addr::tests::origin_is_exact_at_huge_depth |
| NAV-02 | Pan | Right-drag, two fingers, Tool button › Pan | W D M | rust:camera::tests::point_stays_put_under_cursor_at_depth, todo |
| NAV-03 | Home: fly back to where the canvas starts | Menu › Home; Menu › Views › Home; ⌂ on the folded bookmarks bar | W D M | ui:tools-tour, ui:zoom-home |
| NAV-04 | Views: save a view, fly back to it, rename, delete | Menu › Views | W D M | ui:bookmarks, rust:snapshot::tests::roundtrip_keeps_strokes_timeline_bookmarks_and_view |
| NAV-05 | Views folded into a bar (Home first, initials, + to save) | Menu › Views › fold (⌃) | W | ui:bookmarks |
| NAV-06 | Search text and fly to it | Menu › Search text | W D M | rust:search::tests::finds_case_insensitively_with_excerpt, todo |
| NAV-07 | Timeline: scrub and replay the drawing, restore an older state, bookmark a moment | Menu › Timeline (its Bookmark button keeps the moment); Menu › Views flies back to it | W D M | ui:timeline-bookmark, ui:tools-tour, rust:timeline::tests::replays_draws_and_erases, rust:timeline::tests::window_leaves_out_older_ink |
| NAV-08 | Portals: a shape or outline that shows a saved view; zoom through it | Tool button › Portal | W D M | rust:portal::tests::a_portal_shows_its_view_at_every_zoom, rust:portal::tests::zooming_down_the_endless_street_goes_round, rust:portal::tests::portals_survive_saving_and_old_apps_see_a_shape |
| NAV-09 | Endless street demo (infinite zoom loop through a portal) | Try mode › Menu › Views › Endless street | W | rust:portal::tests::zooming_down_the_endless_street_goes_round, todo |
| NAV-10 | Public views: make a saved view public and everyone on the page sees it (with its author), flies to it or keeps a copy; a page server keeps them | Menu › Views › ○ Public | W S | ui:public-views, rust:views::tests::views_merge_by_id |
| NAV-11 | Rulers along the top and left edges in home-view points at any zoom, with the selection's extent marked | Menu › Rulers | W D M | ui:measure, rust:measure::tests::ticks_are_round |

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
| WS-04 | Workspace changes by role: new page / folder, rename, move, delete | Menu › Pages › select a server, folder or page › the bar (New page, New folder, Rename, Move to, Delete); drag a page onto a folder to move it | W D M | ui:workspace, rust:hub::tests::directory_accounts_decide_what_the_page_list_gives, ui:drag-upload |
| WS-05 | Upload a page into a workspace: the open one into a folder, or any page on this device | Menu › Pages › select a server or folder › Upload open page; or select a page on This device › Upload; or drag a page from This device onto a server or folder | W D M | ui:workspace, ui:upload-local, ui:drag-upload |
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
| LIVE-08 | People strip: a badge for each person on a live page (right side in landscape, across the top in portrait); tap to go to their view, right click (press and hold) to follow or stop | the badges, top right | W D M | ui:people-strip |

## Page server (workspace host)

| ID | Feature | Where | Platforms | Tests |
|---|---|---|---|---|
| SRV-01 | Serve pages to the apps (`og-paper --serve-dir DIR`) | command line; Docker on go | S | rust:hub::tests::directory_accounts_decide_what_the_page_list_gives |
| SRV-02 | Home page: how to connect, links, downloads for every client | the server's address in a browser | S | rust:hubpage::tests::the_page_shows_the_view_link_but_never_the_full_one_without_the_key |
| SRV-03 | Console sign-in (first account admin / password), tabs greyed until signed in | server › Sign in | S | rust:hubconsole::tests::roles_decide_what_the_console_allows |
| SRV-04 | Console pages as folders: make, rename, move, delete (tagged), restore | server › Pages | S | rust:hubconsole::tests::roles_decide_what_the_console_allows |
| SRV-05 | Users and roles: admin, subadmin, user, viewer; reset and change passwords | server › Users; server › Password | S | rust:hubconsole::tests::roles_decide_what_the_console_allows |
| SRV-06 | Automation API (JSON over WebSocket at /api; JS and Python clients) | `ws://server/api` | S | todo |
| SRV-08 | AI assistants over MCP (`/mcp`): list, read, look at, draw on and edit pages; design and review prompts; the view key gives read-only tools | `http://server/mcp?k=…` (see docs/MCP.md) | S | rust:mcp::tests::keys_decide_who_gets_in_and_what_they_may_do, rust:mcp::tests::it_speaks_the_protocol, rust:mcp::tests::tools_become_api_requests |
| SRV-09 | Commands for scripts, plugins and assistants: things listed with ids and boxes, a picture of a page, move / set / order / delete by id, tables | `get: objects`, `get: picture`, `move`, `set`, `order`, `delete`, `add: table` | W D S | rust:script::tests::things_added_are_listed_with_ids_and_boxes, rust:script::tests::edits_move_restyle_reorder_and_delete_by_id, rust:script::tests::a_picture_shows_the_page |
| SRV-07 | Signed in, you get your account's rights: the app signs open pages in with a Pages sign-in (a view-only page can draw at once), and "Open in the web app" from a signed-in browser opens with that account | Pages › server › Sign in; the server's home page | W D M S | ui:sign-in-upgrade, rust:hub::tests::page_sign_in_gives_drawing_rights_but_not_to_viewers, rust:hubpage::tests::signed_in_the_web_app_link_carries_the_account |

## Layout and settings

| ID | Feature | Where | Platforms | Tests |
|---|---|---|---|---|
| UI-01 | Menu (gear, top right) grouped in sections | gear button | W D M | ui:menu |
| UI-02 | Tool fan (undo, redo, tools) | Tool button (bottom right) | W D M | ui:draw-undo |
| UI-03 | Quick toolbar with numbered slots and a hold slot per toolbar (the main toolbar’s is in use; tap it to use its tool; set it from the inventory) | Menu › UI settings › Quick toolbar | W D M | ui:tools-tour, ui:hold-slot, rust:hotbar::tests::slots_round_trip, rust:hotbar::tests::names_and_many_toolbars_survive |
| UI-04 | Inventory: drag tools between slots (on a phone, toolbars past what fits beside it scroll sideways so it stays on the screen); drag onto the trash to delete; basic tools (undo, redo and every tool at its defaults, as the tool menu has them, then every plugin's tools; wrapping onto more lines as needed) to drag or tap onto a toolbar or the inventory, folded away with its title | Quick toolbar › bag | W D M | rust:hotbar::tests::inventory_keeps_a_free_row, todo, ui:basic-tools |
| UI-18 | Inventory: Views (the canvas's saved views) to drag or tap into any slot and fly there with a tap; folds away with its title; its + saves the current view under a name you give | Toolbars › inventory › Views | W D M | ui:quickbar-extras, ui:views-save, rust:hotbar::tests::quick_bar_extras_survive_in_order |
| UI-19 | Quick bar "+": drop a tool, plugin tool or view on it for another slot past the toolbar (tap only, no key); drag one onto 🗑 to remove it; right click (press and hold) for Add this view as a bookmark, Add the current tool or Remove the last extra slot; an extra slot's own menu has Remove from the quick bar | Quick bar › + (right click: more) | W D M | ui:quickbar-extras, rust:hotbar::tests::quick_bar_extras_survive_in_order |
| UI-20 | Swatches: saved colors under every color dial, shared by every tool and canvas; + keeps the color in use, a tap takes one, right click or long press removes it | Any color dial › Swatches | W D M | ui:gradients, rust:swatches::tests::swatches_keep_order_once_each |
| UI-05 | Radial toolbar; page shaded only under an open fan | Menu › UI settings › Radial toolbar | W D M | rust:layout::tests::fans_open_toward_the_middle, todo |
| UI-06 | Edit layout (move the buttons) | Menu › UI settings › Edit layout | W D M | rust:layout::tests::layouts_round_trip, todo |
| UI-07 | UI size | Menu › UI settings › UI size | W D M | todo |
| UI-08 | Hide helper text | Menu › UI settings › Hints | W D M | todo |
| UI-09 | Dark mode and grid | Menu › UI settings | W D M | todo |
| UI-10 | Hotkeys (view and change) | Menu › Hotkeys | W D | rust:hotkeys::tests::defaults_have_no_clashes, rust:hotkeys::tests::keymap_changes_round_trip_and_keys_move |
| UI-11 | Full screen | Menu › Full screen | W | todo |
| UI-12 | Movable cards (drag the title; double-tap to reset) | any card's title bar | W | ui:bookmarks |
| UI-13 | Phone keyboard for the app's text boxes | tap any text box on a phone | W | ui:share-live |
| UI-14 | Paste, copy and cut go to the focused text box | Ctrl+V in a text box | W D | ui:share-live |
| UI-15 | Try mode tour | omegagiven.github.io/OG-paper/try/ › Tour | W | todo |
| UI-16 | Tool panel: a sheet along the bottom on phones (drag it taller or tuck it into a strip), a movable, resizable window on wider screens (wide: color in its own column); width and opacity always in view, the rest in tabs | tap the strip / the panel button | W D M | ui:tool-panel |
| UI-17 | Show frame time: frames per second and time per frame, to report slowness | Menu › UI settings › Show frame time | W D M | ui:frame-time |

## Plugins

| ID | Feature | Where | Platforms | Tests |
|---|---|---|---|---|
| PLUG-01 | WASM plugins | Menu › Plugins | W D | rust:plugin::tests::the_example_plugin_loads_and_answers |
| PLUG-04 | Plugins' tools in the tool menu, the basic tools and toolbar slots (icons; placing tools wait for a tap), plugin permissions (asked for, allowed in Plugins) and audio clips: the Audio notes plugin's Voice note tool records a note where you tap, into a clip anyone on the page can play | Menu › Plugins › Install Audio notes › Allow; Tools › Voice note | W | ui:audio-notes, rust:plugin::tests::audio_notes_asks_for_the_microphone, rust:hotbar::tests::a_plugin_tool_in_a_slot_survives, rust:audio::tests::base64_and_clock |
| PLUG-02 | .ogpack packs (toolbars, tools, stickers) | Menu › Plugins › Install pack | W D | rust:pack::tests::packs_read_toolbars_tools_and_stickers |
| PLUG-03 | Scripts (automation inside the app) | Menu › Plugins; `window.ogPaper.run` | W D | rust:script::tests::colors_parse, todo |

## Engine (no UI)

| ID | Feature | Where | Platforms | Tests |
|---|---|---|---|---|
| ENG-01 | Millions of strokes stay fast (visible set bounded) | — | W D M | rust:tests::million_strokes_visible_set_stays_bounded, rust:visible::tests::query_matches_brute_force_under_random_pan_and_zoom |
| ENG-02 | File format round trips (deep cells, tombstones, sync log) | — | W D M | rust:tests::roundtrip_with_deep_cells_and_tombstones, rust:tests::sync_log_and_canvas_id_round_trip |
| ENG-03 | GPU shaders validate | — | W D M | rust:render::tests::shader_validates |
