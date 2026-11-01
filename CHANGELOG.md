# Changelog

All notable changes are documented here. The format follows [Conventional Commits](https://www.conventionalcommits.org/) and the version scheme is [Semantic Versioning](https://semver.org/).

## [1.0.4](https://github.com/khasky/emploidle/compare/v1.0.3...v1.0.4) (2026-11-01)


### Features

* **activity:** add weekday scheduling ([d1d45d5](https://github.com/khasky/emploidle/commit/d1d45d5c7f35d0178a732c1373367f27970e08ae))


### Bug Fixes

* **activity:** rotate complete app and tab scenarios ([7d2551b](https://github.com/khasky/emploidle/commit/7d2551b181401f09db24bc62a1d3b5f55e91aa70))

### Known limitations

* During an in-page address change, a browser scroll command already sent through Windows UI Automation may finish after the transition. Preserving the new page's scroll position and read state is not guaranteed for this case.

## [1.0.3](https://github.com/khasky/emploidle/compare/v1.0.2...v1.0.3) (2026-10-31)


### Features

* **activity:** add tray states and varied editor activity ([4fca777](https://github.com/khasky/emploidle/commit/4fca77724513b968ebaf6ef8298c28b63b2823d3))


### Bug Fixes

* **tray:** use fixed-size pixel chunks ([a435370](https://github.com/khasky/emploidle/commit/a435370a7f7826f1d763ab9a1ca3b9f9ac77f791))

## [1.0.2](https://github.com/khasky/emploidle/compare/v1.0.1...v1.0.2) (2026-10-31)


### Features

* automate desktop activity and simplify the interface ([18ffe4f](https://github.com/khasky/emploidle/commit/18ffe4f4167e4bfc980c133b081b9b78df17d988))
* **tauri:** add signed application updates ([6e57b7f](https://github.com/khasky/emploidle/commit/6e57b7facf185910c14efc9b7a2c237f95de17f8))
* **ui:** add the Emploidle application icon ([68bc94f](https://github.com/khasky/emploidle/commit/68bc94f609c58956121bc7c12d9a5ff08b27d8b0))


### Bug Fixes

* **ui:** fill square application and tray icons ([0ed18e3](https://github.com/khasky/emploidle/commit/0ed18e3659ac8c4c7520044db678c35caf94cad8))

## [1.0.1](https://github.com/khasky/emploidle/compare/v1.0.0...v1.0.1) (2026-10-31)


### Features

* **activity:** add scenarios and daily schedules ([e2baaa5](https://github.com/khasky/emploidle/commit/e2baaa541829e6236655348f8ef55c247f56454e))


### Bug Fixes

* **activity:** pause while mouse buttons are held ([19397dc](https://github.com/khasky/emploidle/commit/19397dccfa028905f57b0c2ad9bbce45c2d189e5))
* **activity:** skip inactive reading windows ([dda66d4](https://github.com/khasky/emploidle/commit/dda66d4cd9c02aa9a8250403c6d9e156bfdfe229))
* **platform:** retain AppImage path for startup ([e90d2f3](https://github.com/khasky/emploidle/commit/e90d2f3749c9bf41cb5f969851e16d33633c122e))

## 1.0.0 (2026-10-30)


### Features

* add initial version ([421eea3](https://github.com/khasky/emploidle/commit/421eea3072b70ff95a9c42308262101376c0920c))
