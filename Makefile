.PHONY: build run app dmg clean

build:
	swift build

run:
	swift run Mouseless

app:
	./Scripts/build_app.sh

dmg:
	./Scripts/build_dmg.sh

clean:
	swift package clean
	rm -rf .build/release/Mouseless.app .build/release/Mouseless.dmg
