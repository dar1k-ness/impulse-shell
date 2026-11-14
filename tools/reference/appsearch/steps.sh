# Filesystem steps of the launcher app-search scenario (crates/qml/tests/appsearch/shell.qml
# driving the tree's LauncherSearch), one per line: `name<TAB>shell command` run with cwd = scenario
# base, XDG_DATA_HOME=base/home and XDG_DATA_DIRS=base/sys. foot.desktop exists in both dirs (same
# id), rescans come from parent-directory and file touches. Shared by record-qs.sh and the Rust test.
setup	mkdir -p home/applications sys/applications && printf '[Desktop Entry]\nType=Application\nExec=true\nName=Foot\n' > sys/applications/foot.desktop && printf '[Desktop Entry]\nType=Application\nExec=true\nName=Foot\n' > home/applications/foot.desktop && printf '[Desktop Entry]\nType=Application\nExec=true\nName=Firefox\n' > sys/applications/firefox.desktop && printf '[Desktop Entry]\nType=Application\nExec=true\nName=kitty\n' > sys/applications/kitty.desktop
home_touch	touch home/x && rm home/x
home_touch2	touch home/x && rm home/x
override_touch	touch home/applications/foot.desktop
dup_atomic	printf '[Desktop Entry]\nType=Application\nExec=true\nName=Foot\n' > sys/applications/.foot.tmp && mv sys/applications/.foot.tmp sys/applications/foot.desktop
actions_two_inplace	printf '[Desktop Entry]\nType=Application\nExec=true\nName=Firefox\nActions=new-window;private;\n\n[Desktop Action new-window]\nName=Act-new-window\nExec=true\n\n[Desktop Action private]\nName=Act-private\nExec=true\n' > sys/applications/firefox.desktop
rescan	touch home/x && rm home/x
actions_drop	printf '[Desktop Entry]\nType=Application\nExec=true\nName=Firefox\nActions=new-window;\n\n[Desktop Action new-window]\nName=Act-new-window\nExec=true\n' > sys/applications/firefox.desktop && touch home/x && rm home/x
actions_rename	printf '[Desktop Entry]\nType=Application\nExec=true\nName=Firefox\nActions=profile;\n\n[Desktop Action profile]\nName=Act-profile\nExec=true\n' > sys/applications/firefox.desktop && touch home/x && rm home/x
actions_none	printf '[Desktop Entry]\nType=Application\nExec=true\nName=Firefox\nActions=;\n' > sys/applications/firefox.desktop && touch home/x && rm home/x
install	printf '[Desktop Entry]\nType=Application\nExec=true\nName=Kitten\n' > home/applications/kitten.desktop
uninstall	rm sys/applications/kitty.desktop
mask	printf '[Desktop Entry]\nType=Application\nExec=true\nName=Foot\nHidden=true\n' > home/applications/.f.tmp && mv home/applications/.f.tmp home/applications/foot.desktop
unmask	rm home/applications/foot.desktop
nodisplay	printf '[Desktop Entry]\nType=Application\nExec=true\nName=Firefox\nNoDisplay=true\n' > sys/applications/.ff.tmp && mv sys/applications/.ff.tmp sys/applications/firefox.desktop
redisplay	printf '[Desktop Entry]\nType=Application\nExec=true\nName=Firefox\n' > sys/applications/.ff.tmp && mv sys/applications/.ff.tmp sys/applications/firefox.desktop
burst	for k in 1 2 3 4 5 6 7 8 9 10; do touch home/applications/foot.desktop sys/applications/foot.desktop; touch home/b$k; done; rm -f home/b*
reinstall	printf '[Desktop Entry]\nType=Application\nExec=true\nName=kitty\n' > sys/applications/kitty.desktop
home_touch3	touch home/x && rm home/x
