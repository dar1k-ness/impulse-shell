137 rows: 137 pass, 0 differ, 0 tool errors; exit code 0

`coarse`: pixels whose colour differs by more than 24 of 255 after both shots are averaged over 5x5 pixels, outside the masks. A shot passes when `coarse` <= `limit`.

- limit 0 for `*`: a still screen: two runs of the original measure 0 on every shot, and so does native on every shot without a defect
- limit 12000 for `*.f[0-9]*`: a frame of a running animation, matched to the closest frame of the original: the phase differs between any two runs (up to about 4000 between two runs of the original, up to about 9000 against native); a frame that shows something else measures 100000 and more

| scene | status | coarse | limit | px differ | >8 | >32 | max | regions (x,y,w,h) | note |
|---|---|---|---|---|---|---|---|---|---|
| overlay-actions.log | pass |  |  |  |  |  |  |  |  |
| state.log | pass |  |  |  |  |  |  |  |  |
| anim_overview.f00 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_overview.f01 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_overview.f02 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_overview.f03 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_overview.f04 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_overview.f05 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_overview.f06 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_overview.f07 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_overview.f08 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_overview.f09 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_overview.f10 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_overview.f11 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_session.f00 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_session.f01 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_session.f02 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_session.f03 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_session.f04 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_session.f05 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_session.f06 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_session.f07 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_session.f08 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_session.f09 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_session.f10 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_session.f11 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_sidebarRight.f00 | pass | 0 | 12000 | 0 | 0 | 0 | 0 |  |  |
| anim_sidebarRight.f01 | pass | 0 | 12000 | 1075 | 0 | 0 | 5 |  |  |
| anim_sidebarRight.f02 | pass | 0 | 12000 | 1075 | 0 | 0 | 5 |  |  |
| anim_sidebarRight.f03 | pass | 0 | 12000 | 74 | 0 | 0 | 7 |  |  |
| anim_sidebarRight.f04 | pass | 0 | 12000 | 118 | 0 | 0 | 7 |  |  |
| anim_sidebarRight.f05 | pass | 0 | 12000 | 1103 | 0 | 0 | 7 |  |  |
| anim_sidebarRight.f06 | pass | 0 | 12000 | 1169 | 0 | 0 | 7 |  |  |
| anim_sidebarRight.f07 | pass | 0 | 12000 | 1169 | 0 | 0 | 7 |  |  |
| anim_sidebarRight.f08 | pass | 0 | 12000 | 1169 | 0 | 0 | 7 |  |  |
| anim_sidebarRight.f09 | pass | 0 | 12000 | 1169 | 0 | 0 | 7 |  |  |
| anim_sidebarRight.f10 | pass | 0 | 12000 | 1169 | 0 | 0 | 7 |  |  |
| anim_sidebarRight.f11 | pass | 0 | 12000 | 1169 | 0 | 0 | 7 |  |  |
| bar_hover_clock | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| bar_hover_leftSide | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| bar_hover_media | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| bar_hover_resources | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| bar_hover_rightSide | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| bar_hover_utilButtons | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| bar_hover_workspaces | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| bar_vertical | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| bar_vertical_live | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| bar_vertical_sidebarRight | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| base | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| cheatsheet | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| firstStart | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| firstStart_sidebarRight | pass | 0 | 0 | 87 | 0 | 0 | 7 |  |  |
| killDialog | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| lock | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| lock_after_unlock | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| lock_typing | pass | 0 | 0 | 284 | 0 | 0 | 1 |  |  |
| mediaControls | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| notif_popup | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| notif_popup2 | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| notif_sidebar | pass | 0 | 0 | 74 | 0 | 0 | 7 |  |  |
| notifburst.f0 | pass | 77 | 12000 | 646 | 570 | 325 | 200 | 1648,80,64,32; 1760,96,16,16 |  |
| notifburst.f1 | pass | 217 | 12000 | 1158 | 1013 | 580 | 150 | 1648,80,144,32 |  |
| notifburst.f2 | pass | 740 | 12000 | 3584 | 2524 | 2085 | 202 | 1568,64,144,48; 1792,64,64,48 |  |
| notifburst.f3 | pass | 524 | 12000 | 3424 | 2350 | 1865 | 202 | 1568,64,96,48; 1808,64,48,48 |  |
| notifburst.f4 | pass | 267 | 12000 | 3308 | 2207 | 1731 | 202 | 1568,64,80,48; 1808,80,16,32; 1840,80,16,16 |  |
| notifburst.f5 | pass | 267 | 12000 | 3308 | 2207 | 1731 | 202 | 1568,64,80,48; 1808,80,16,32; 1840,80,16,16 |  |
| notifburst_end | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| notifburst_sidebar | pass | 0 | 0 | 87 | 0 | 0 | 7 |  |  |
| osdVolume | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| overlay | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| overview | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| polkit | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| polkit_unfocused | pass | 0 | 0 | 3 | 0 | 0 | 2 |  |  |
| polkit_wrong | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_notif_popup_expanded | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_notif_popup_hover | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_notif_sidebar_dragged | pass | 0 | 0 | 105 | 0 | 0 | 7 |  |  |
| ptr_notif_sidebar_expanded | pass | 0 | 0 | 105 | 0 | 0 | 7 |  |  |
| ptr_notif_sidebar_hover | pass | 0 | 0 | 265 | 0 | 0 | 7 |  |  |
| ptr_overlay_btn_center | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overlay_btn_folder | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overlay_btn_region | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overlay_btn_regionRecord | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overlay_btn_screenRecord | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overlay_btn_screenshot | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overlay_btn_task0 | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overlay_btn_task1 | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overlay_btn_task2 | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overlay_btn_task3 | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overlay_btn_task4 | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overlay_btn_task5 | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overlay_btn_task6 | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overlay_crosshair | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overlay_mixer | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overlay_pinned | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overview_hover_searchIcon | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overview_hover_ws3 | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overview_ws1 | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_overview_ws2 | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_sidebarLeft_anime | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_sidebarLeft_hover_clear | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_sidebarLeft_intelligence | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_sidebarRight_calCollapse | pass | 0 | 0 | 265 | 0 | 0 | 7 |  |  |
| ptr_sidebarRight_calExpand | pass | 0 | 0 | 265 | 0 | 0 | 7 |  |  |
| ptr_sidebarRight_calNext | pass | 0 | 0 | 21 | 0 | 0 | 2 |  |  |
| ptr_sidebarRight_calPrev | pass | 0 | 0 | 15 | 0 | 0 | 2 |  |  |
| ptr_sidebarRight_editMode | pass | 0 | 0 | 265 | 0 | 0 | 7 |  |  |
| ptr_sidebarRight_editModeOff | pass | 0 | 0 | 265 | 0 | 0 | 7 |  |  |
| ptr_sidebarRight_hover_bluetooth | pass | 0 | 0 | 21 | 0 | 0 | 2 |  |  |
| ptr_sidebarRight_hover_calNext | pass | 0 | 0 | 21 | 0 | 0 | 2 |  |  |
| ptr_sidebarRight_hover_clearAll | pass | 0 | 0 | 21 | 0 | 0 | 2 |  |  |
| ptr_sidebarRight_hover_internet | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| ptr_sidebarRight_hover_silence | pass | 0 | 0 | 21 | 0 | 0 | 2 |  |  |
| region_record | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| region_screenshot | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| sameLogin_sidebarRight | pass | 0 | 0 | 87 | 0 | 0 | 7 |  |  |
| search | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| search_query | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| session | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| settings | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| sidebarLeft | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| sidebarRight | pass | 0 | 0 | 21 | 0 | 0 | 2 |  |  |
| sidebarRight_calendar | pass | 0 | 0 | 21 | 0 | 0 | 2 |  |  |
| sidebarRight_timer | pass | 0 | 0 | 21 | 0 | 0 | 2 |  |  |
| sidebarRight_todo | pass | 0 | 0 | 21 | 0 | 0 | 2 |  |  |
| waffle | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| waffle_actionCenter | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| waffle_hover_clock | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| waffle_hover_search | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| waffle_hover_start | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| waffle_live | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| waffle_notificationCenter | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| waffle_osd | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| waffle_startMenu | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| waffle_taskView | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| wallpaperSelector | pass | 0 | 0 | 0 | 0 | 0 | 0 |  |  |
| welcome | pass | 0 | 0 | 1 | 0 | 0 | 1 |  | screen still changing when shot: a,b |
