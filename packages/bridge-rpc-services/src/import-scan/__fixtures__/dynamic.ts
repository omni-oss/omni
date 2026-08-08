// A non-literal dynamic import: the specifier is a variable, so the lexer
// cannot know the target and the scanner flags it as a diagnostic (the caller
// must grant an explicit read root for it).
const mod = "./cycle-a";
export const loaded = import(mod);
