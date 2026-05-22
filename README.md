# GGO Launcher

GGO Custom Edition 전용 런처. Tauri 2 (Rust + React + TypeScript) 기반 단일 실행 파일.

## 개발 실행

```powershell
npm install
npm run tauri dev
```

## 릴리스 빌드

```powershell
npm run tauri build
```

산출물: `src-tauri/target/release/GGOLauncher.exe`

## 프로젝트 구조

```
src/              프론트엔드 (React + TS)
src-tauri/        백엔드 (Rust)
public/           정적 자산 (로고 등)
```
