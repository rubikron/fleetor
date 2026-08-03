/// <reference types="vite/client" />
//
// Pulls in Vite's ambient types — specifically `import.meta.env`, which the
// dev-only frame probe uses to gate itself out of production builds.
// tsconfig sets `"types": []`, which suppresses automatic @types discovery but
// not an explicit triple-slash reference like this one.
