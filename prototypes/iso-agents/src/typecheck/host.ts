import ts from "typescript";
import { GUEST_GLOBALS, GUEST_GLOBALS_TEXT, LIB_TEXT, memoSourceFile } from "./vfs";

// Guest-check compiler options. NEVER add a `lib` option: with `lib` set,
// typescript routes lib acquisition through pathForLibFile() (raw option
// string + directory prefix) and never calls getDefaultLibFileName(), so a
// bare-name VFS map serves zero libs and every check drowns in TS2318/
// TS2583. Verified in typescript@6.0.3 (ts.js L127725-L127749).
export const GUEST_COMPILER_OPTIONS: ts.CompilerOptions = {
  strict: true,
  target: ts.ScriptTarget.ES2022,
  noEmit: true,
  module: ts.ModuleKind.ESNext,
  moduleResolution: ts.ModuleResolutionKind.Bundler,
  skipLibCheck: true,
  isolatedModules: true,
  forceConsistentCasingInFileNames: true,
};

// Design A host (research-verified in workerd): every file — libs included —
// is served from maps; nothing touches a host filesystem. Lib + globals
// SourceFiles are memoized; guest files re-parse per check. Guest paths are
// rooted ("/guest.ts"); lib keys are bare ("lib.es2022.d.ts").
export function makeHost(guestFiles: Map<string, string>): ts.CompilerHost {
  return {
    getSourceFile(fileName, languageVersionOrOptions) {
      if (fileName === GUEST_GLOBALS) {
        return memoSourceFile(fileName, GUEST_GLOBALS_TEXT, languageVersionOrOptions);
      }
      const libText = LIB_TEXT.get(fileName);
      if (libText !== undefined) {
        return memoSourceFile(fileName, libText, languageVersionOrOptions);
      }
      const text = guestFiles.get(fileName);
      return text === undefined
        ? undefined
        : ts.createSourceFile(fileName, text, languageVersionOrOptions, true);
    },
    getDefaultLibFileName: () => "lib.es2022.d.ts", // MUST be a LIB_TEXT key
    writeFile: () => {}, // noEmit: nothing to write, ever
    getCurrentDirectory: () => "/",
    getDirectories: () => [],
    fileExists: (f) => f === GUEST_GLOBALS || LIB_TEXT.has(f) || guestFiles.has(f),
    readFile: (f) => LIB_TEXT.get(f) ?? guestFiles.get(f),
    readDirectory: () => [],
    getCanonicalFileName: (f) => f,
    useCaseSensitiveFileNames: () => true,
    getNewLine: () => "\n",
  };
}
