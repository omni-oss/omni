// A nested run-generator over `gen-a`, modelled by the `gen:` specifier scheme
// the scanner treats as a generator boundary (not a plain file import). This
// module is never executed by the scanner, so the specifier need not be a real
// loadable module.
import "gen:./gen-a";
export default function genB(): void {}
