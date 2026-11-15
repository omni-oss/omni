import Ws from "node:fs/promises";
class ee extends Error {
    constructor(e, t, r) {
        super(r);
        (Error.captureStackTrace(this, this.constructor),
            (this.name = this.constructor.name),
            (this.code = t),
            (this.exitCode = e),
            (this.nestedError = void 0));
    }
}
class oe extends ee {
    constructor(e) {
        super(1, "commander.invalidArgument", e);
        (Error.captureStackTrace(this, this.constructor),
            (this.name = this.constructor.name));
    }
}
class Pe {
    constructor(e, t) {
        switch (
            ((this.description = t || ""),
            (this.variadic = !1),
            (this.parseArg = void 0),
            (this.defaultValue = void 0),
            (this.defaultValueDescription = void 0),
            (this.argChoices = void 0),
            e[0])
        ) {
            case "<":
                ((this.required = !0), (this._name = e.slice(1, -1)));
                break;
            case "[":
                ((this.required = !1), (this._name = e.slice(1, -1)));
                break;
            default:
                ((this.required = !0), (this._name = e));
                break;
        }
        if (this._name.endsWith("..."))
            ((this.variadic = !0), (this._name = this._name.slice(0, -3)));
    }
    name() {
        return this._name;
    }
    _collectValue(e, t) {
        if (t === this.defaultValue || !Array.isArray(t)) return [e];
        return (t.push(e), t);
    }
    default(e, t) {
        return (
            (this.defaultValue = e), (this.defaultValueDescription = t), this
        );
    }
    argParser(e) {
        return ((this.parseArg = e), this);
    }
    choices(e) {
        return (
            (this.argChoices = e.slice()),
            (this.parseArg = (t, r) => {
                if (!this.argChoices.includes(t))
                    throw new oe(
                        `Allowed choices are ${this.argChoices.join(", ")}.`,
                    );
                if (this.variadic) return this._collectValue(t, r);
                return t;
            }),
            this
        );
    }
    argRequired() {
        return ((this.required = !0), this);
    }
    argOptional() {
        return ((this.required = !1), this);
    }
}
function Ze(e) {
    let t = e.name() + (e.variadic === !0 ? "..." : "");
    return e.required ? "<" + t + ">" : "[" + t + "]";
}
import mt from "node:child_process";
import { EventEmitter as Ys } from "node:events";
import Ce from "node:fs";
import B from "node:path";
import b from "node:process";
import { stripVTControlCharacters as Qs } from "node:util";
import { stripVTControlCharacters as Ks } from "node:util";
class Ae {
    constructor() {
        ((this.helpWidth = void 0),
            (this.minWidthToWrap = 40),
            (this.sortSubcommands = !1),
            (this.sortOptions = !1),
            (this.showGlobalOptions = !1));
    }
    prepareContext(e) {
        this.helpWidth = this.helpWidth ?? e.helpWidth ?? 80;
    }
    visibleCommands(e) {
        let t = e.commands.filter((n) => !n._hidden),
            r = e._getHelpCommand();
        if (r && !r._hidden) t.push(r);
        if (this.sortSubcommands)
            t.sort((n, o) => n.name().localeCompare(o.name()));
        return t;
    }
    compareOptions(e, t) {
        let r = (n) =>
            n.short ? n.short.replace(/^-/, "") : n.long.replace(/^--/, "");
        return r(e).localeCompare(r(t));
    }
    visibleOptions(e) {
        let t = e.options.filter((n) => !n.hidden),
            r = e._getHelpOption();
        if (r && !r.hidden) {
            let n = r.short && e._findOption(r.short),
                o = r.long && e._findOption(r.long);
            if (!n && !o) t.push(r);
            else if (r.long && !o)
                t.push(
                    e
                        .createOption(r.long, r.description)
                        .helpGroup(r.helpGroupHeading),
                );
            else if (r.short && !n)
                t.push(
                    e
                        .createOption(r.short, r.description)
                        .helpGroup(r.helpGroupHeading),
                );
        }
        if (this.sortOptions) t.sort(this.compareOptions);
        return t;
    }
    visibleGlobalOptions(e) {
        if (!this.showGlobalOptions) return [];
        let t = [];
        for (let r = e.parent; r; r = r.parent) {
            let n = r.options.filter((o) => !o.hidden);
            t.push(...n);
        }
        if (this.sortOptions) t.sort(this.compareOptions);
        return t;
    }
    visibleArguments(e) {
        if (e._argsDescription)
            e.registeredArguments.forEach((t) => {
                t.description =
                    t.description || e._argsDescription[t.name()] || "";
            });
        if (e.registeredArguments.find((t) => t.description))
            return e.registeredArguments;
        return [];
    }
    subcommandTerm(e) {
        let t = e.registeredArguments.map((r) => Ze(r)).join(" ");
        return (
            e._name +
            (e._aliases[0] ? "|" + e._aliases[0] : "") +
            (e.options.length ? " [options]" : "") +
            (t ? " " + t : "")
        );
    }
    optionTerm(e) {
        return e.flags;
    }
    argumentTerm(e) {
        return e.name();
    }
    longestSubcommandTermLength(e, t) {
        return t
            .visibleCommands(e)
            .reduce(
                (r, n) =>
                    Math.max(
                        r,
                        this.displayWidth(
                            t.styleSubcommandTerm(t.subcommandTerm(n)),
                        ),
                    ),
                0,
            );
    }
    longestOptionTermLength(e, t) {
        return t
            .visibleOptions(e)
            .reduce(
                (r, n) =>
                    Math.max(
                        r,
                        this.displayWidth(t.styleOptionTerm(t.optionTerm(n))),
                    ),
                0,
            );
    }
    longestGlobalOptionTermLength(e, t) {
        return t
            .visibleGlobalOptions(e)
            .reduce(
                (r, n) =>
                    Math.max(
                        r,
                        this.displayWidth(t.styleOptionTerm(t.optionTerm(n))),
                    ),
                0,
            );
    }
    longestArgumentTermLength(e, t) {
        return t
            .visibleArguments(e)
            .reduce(
                (r, n) =>
                    Math.max(
                        r,
                        this.displayWidth(
                            t.styleArgumentTerm(t.argumentTerm(n)),
                        ),
                    ),
                0,
            );
    }
    commandUsage(e) {
        let t = e._name;
        if (e._aliases[0]) t = t + "|" + e._aliases[0];
        let r = "";
        for (let n = e.parent; n; n = n.parent) r = n.name() + " " + r;
        return r + t + " " + e.usage();
    }
    commandDescription(e) {
        return e.description();
    }
    subcommandDescription(e) {
        return e.summary() || e.description();
    }
    optionDescription(e) {
        let t = [];
        if (e.argChoices)
            t.push(
                `choices: ${e.argChoices.map((r) => JSON.stringify(r)).join(", ")}`,
            );
        if (e.defaultValue !== void 0) {
            if (
                e.required ||
                e.optional ||
                (e.isBoolean() && typeof e.defaultValue === "boolean")
            )
                t.push(
                    `default: ${e.defaultValueDescription || JSON.stringify(e.defaultValue)}`,
                );
        }
        if (e.presetArg !== void 0 && e.optional)
            t.push(`preset: ${JSON.stringify(e.presetArg)}`);
        if (e.envVar !== void 0) t.push(`env: ${e.envVar}`);
        if (t.length > 0) {
            let r = `(${t.join(", ")})`;
            if (e.description) return `${e.description} ${r}`;
            return r;
        }
        return e.description;
    }
    argumentDescription(e) {
        let t = [];
        if (e.argChoices)
            t.push(
                `choices: ${e.argChoices.map((r) => JSON.stringify(r)).join(", ")}`,
            );
        if (e.defaultValue !== void 0)
            t.push(
                `default: ${e.defaultValueDescription || JSON.stringify(e.defaultValue)}`,
            );
        if (t.length > 0) {
            let r = `(${t.join(", ")})`;
            if (e.description) return `${e.description} ${r}`;
            return r;
        }
        return e.description;
    }
    formatItemList(e, t, r) {
        if (t.length === 0) return [];
        return [r.styleTitle(e), ...t, ""];
    }
    groupItems(e, t, r) {
        let n = new Map();
        return (
            e.forEach((o) => {
                let i = r(o);
                if (!n.has(i)) n.set(i, []);
            }),
            t.forEach((o) => {
                let i = r(o);
                if (!n.has(i)) n.set(i, []);
                n.get(i).push(o);
            }),
            n
        );
    }
    formatHelp(e, t) {
        let r = t.padWidth(e, t),
            n = t.helpWidth ?? 80;
        function o(l, f) {
            return t.formatItem(l, r, f, t);
        }
        let i = [
                `${t.styleTitle("Usage:")} ${t.styleUsage(t.commandUsage(e))}`,
                "",
            ],
            s = t.commandDescription(e);
        if (s.length > 0)
            i = i.concat([t.boxWrap(t.styleCommandDescription(s), n), ""]);
        let c = t
            .visibleArguments(e)
            .map((l) =>
                o(
                    t.styleArgumentTerm(t.argumentTerm(l)),
                    t.styleArgumentDescription(t.argumentDescription(l)),
                ),
            );
        if (
            ((i = i.concat(this.formatItemList("Arguments:", c, t))),
            this.groupItems(
                e.options,
                t.visibleOptions(e),
                (l) => l.helpGroupHeading ?? "Options:",
            ).forEach((l, f) => {
                let d = l.map((h) =>
                    o(
                        t.styleOptionTerm(t.optionTerm(h)),
                        t.styleOptionDescription(t.optionDescription(h)),
                    ),
                );
                i = i.concat(this.formatItemList(f, d, t));
            }),
            t.showGlobalOptions)
        ) {
            let l = t
                .visibleGlobalOptions(e)
                .map((f) =>
                    o(
                        t.styleOptionTerm(t.optionTerm(f)),
                        t.styleOptionDescription(t.optionDescription(f)),
                    ),
                );
            i = i.concat(this.formatItemList("Global Options:", l, t));
        }
        return (
            this.groupItems(
                e.commands,
                t.visibleCommands(e),
                (l) => l.helpGroup() || "Commands:",
            ).forEach((l, f) => {
                let d = l.map((h) =>
                    o(
                        t.styleSubcommandTerm(t.subcommandTerm(h)),
                        t.styleSubcommandDescription(
                            t.subcommandDescription(h),
                        ),
                    ),
                );
                i = i.concat(this.formatItemList(f, d, t));
            }),
            i.join(`
`)
        );
    }
    displayWidth(e) {
        return Ks(e).length;
    }
    styleTitle(e) {
        return e;
    }
    styleUsage(e) {
        return e
            .split(" ")
            .map((t) => {
                if (t === "[options]") return this.styleOptionText(t);
                if (t === "[command]") return this.styleSubcommandText(t);
                if (t[0] === "[" || t[0] === "<")
                    return this.styleArgumentText(t);
                return this.styleCommandText(t);
            })
            .join(" ");
    }
    styleCommandDescription(e) {
        return this.styleDescriptionText(e);
    }
    styleOptionDescription(e) {
        return this.styleDescriptionText(e);
    }
    styleSubcommandDescription(e) {
        return this.styleDescriptionText(e);
    }
    styleArgumentDescription(e) {
        return this.styleDescriptionText(e);
    }
    styleDescriptionText(e) {
        return e;
    }
    styleOptionTerm(e) {
        return this.styleOptionText(e);
    }
    styleSubcommandTerm(e) {
        return e
            .split(" ")
            .map((t) => {
                if (t === "[options]") return this.styleOptionText(t);
                if (t[0] === "[" || t[0] === "<")
                    return this.styleArgumentText(t);
                return this.styleSubcommandText(t);
            })
            .join(" ");
    }
    styleArgumentTerm(e) {
        return this.styleArgumentText(e);
    }
    styleOptionText(e) {
        return e;
    }
    styleArgumentText(e) {
        return e;
    }
    styleSubcommandText(e) {
        return e;
    }
    styleCommandText(e) {
        return e;
    }
    padWidth(e, t) {
        return Math.max(
            t.longestOptionTermLength(e, t),
            t.longestGlobalOptionTermLength(e, t),
            t.longestSubcommandTermLength(e, t),
            t.longestArgumentTermLength(e, t),
        );
    }
    preformatted(e) {
        return /\n[^\S\r\n]/.test(e);
    }
    formatItem(e, t, r, n) {
        let i = " ".repeat(2);
        if (!r) return i + e;
        let s = e.padEnd(t + e.length - n.displayWidth(e)),
            c = 2,
            u = (this.helpWidth ?? 80) - t - c - 2,
            l;
        if (u < this.minWidthToWrap || n.preformatted(r)) l = r;
        else
            l = n.boxWrap(r, u).replace(
                /\n/g,
                `
` + " ".repeat(t + c),
            );
        return (
            i +
            s +
            " ".repeat(c) +
            l.replace(
                /\n/g,
                `
${i}`,
            )
        );
    }
    boxWrap(e, t) {
        if (t < this.minWidthToWrap) return e;
        let r = e.split(/\r\n|\n/),
            n = /[\s]*[^\s]+/g,
            o = [];
        return (
            r.forEach((i) => {
                let s = i.match(n);
                if (s === null) {
                    o.push("");
                    return;
                }
                let c = [s.shift()],
                    a = this.displayWidth(c[0]);
                (s.forEach((u) => {
                    let l = this.displayWidth(u);
                    if (a + l <= t) {
                        (c.push(u), (a += l));
                        return;
                    }
                    o.push(c.join(""));
                    let f = u.trimStart();
                    ((c = [f]), (a = this.displayWidth(f)));
                }),
                    o.push(c.join("")));
            }),
            o.join(`
`)
        );
    }
}
class de {
    constructor(e, t) {
        ((this.flags = e),
            (this.description = t || ""),
            (this.required = e.includes("<")),
            (this.optional = e.includes("[")),
            (this.variadic = /\p{ID_Continue}\.\.\.[>\]]$/u.test(e)),
            (this.mandatory = !1));
        let r = qs(e);
        if (
            ((this.short = r.shortFlag),
            (this.long = r.longFlag),
            (this.negate = !1),
            this.long)
        )
            this.negate = this.long.startsWith("--no-");
        ((this.defaultValue = void 0),
            (this.defaultValueDescription = void 0),
            (this.presetArg = void 0),
            (this.envVar = void 0),
            (this.parseArg = void 0),
            (this.hidden = !1),
            (this.argChoices = void 0),
            (this.conflictsWith = []),
            (this.implied = void 0),
            (this.helpGroupHeading = void 0));
    }
    default(e, t) {
        return (
            (this.defaultValue = e), (this.defaultValueDescription = t), this
        );
    }
    preset(e) {
        return ((this.presetArg = e), this);
    }
    conflicts(e) {
        return ((this.conflictsWith = this.conflictsWith.concat(e)), this);
    }
    implies(e) {
        let t = e;
        if (typeof e === "string") t = { [e]: !0 };
        return ((this.implied = Object.assign(this.implied || {}, t)), this);
    }
    env(e) {
        return ((this.envVar = e), this);
    }
    argParser(e) {
        return ((this.parseArg = e), this);
    }
    makeOptionMandatory(e = !0) {
        return ((this.mandatory = !!e), this);
    }
    hideHelp(e = !0) {
        return ((this.hidden = !!e), this);
    }
    _collectValue(e, t) {
        if (t === this.defaultValue || !Array.isArray(t)) return [e];
        return (t.push(e), t);
    }
    choices(e) {
        return (
            (this.argChoices = e.slice()),
            (this.parseArg = (t, r) => {
                if (!this.argChoices.includes(t))
                    throw new oe(
                        `Allowed choices are ${this.argChoices.join(", ")}.`,
                    );
                if (this.variadic) return this._collectValue(t, r);
                return t;
            }),
            this
        );
    }
    name() {
        if (this.long) return this.long.replace(/^--/, "");
        return this.short.replace(/^-/, "");
    }
    attributeName() {
        if (this.negate) return xr(this.name().replace(/^no-/, ""));
        return xr(this.name());
    }
    helpGroup(e) {
        return ((this.helpGroupHeading = e), this);
    }
    is(e) {
        return this.short === e || this.long === e;
    }
    isBoolean() {
        return !this.required && !this.optional && !this.negate;
    }
}
class dt {
    constructor(e) {
        ((this.positiveOptions = new Map()),
            (this.negativeOptions = new Map()),
            (this.dualOptions = new Set()),
            e.forEach((t) => {
                if (t.negate) this.negativeOptions.set(t.attributeName(), t);
                else this.positiveOptions.set(t.attributeName(), t);
            }),
            this.negativeOptions.forEach((t, r) => {
                if (this.positiveOptions.has(r)) this.dualOptions.add(r);
            }));
    }
    valueFromOption(e, t) {
        let r = t.attributeName();
        if (!this.dualOptions.has(r)) return !0;
        let n = this.negativeOptions.get(r).presetArg,
            o = n !== void 0 ? n : !1;
        return t.negate === (o === e);
    }
}
function xr(e) {
    return e.split("-").reduce((t, r) => t + r[0].toUpperCase() + r.slice(1));
}
function qs(e) {
    let t,
        r,
        n = /^-[^-]$/,
        o = /^--[^-]/,
        i = e.split(/[ |,]+/).concat("guard");
    if (n.test(i[0])) t = i.shift();
    if (o.test(i[0])) r = i.shift();
    if (!t && n.test(i[0])) t = i.shift();
    if (!t && o.test(i[0])) ((t = r), (r = i.shift()));
    if (i[0].startsWith("-")) {
        let s = i[0],
            c = `option creation failed due to '${s}' in option flags '${e}'`;
        if (/^-[^-][^-]/.test(s))
            throw Error(`${c}
- a short flag is a single dash and a single character
  - either use a single dash and a single character (for a short flag)
  - or use a double dash for a long option (and can have two, like '--ws, --workspace')`);
        if (n.test(s))
            throw Error(`${c}
- too many short flags`);
        if (o.test(s))
            throw Error(`${c}
- too many long flags`);
        throw Error(`${c}
- unrecognised flag format`);
    }
    if (t === void 0 && r === void 0)
        throw Error(`option creation failed due to no flags found in '${e}'.`);
    return { shortFlag: t, longFlag: r };
}
function Xs(e, t) {
    if (Math.abs(e.length - t.length) > 3) return Math.max(e.length, t.length);
    let r = [];
    for (let n = 0; n <= e.length; n++) r[n] = [n];
    for (let n = 0; n <= t.length; n++) r[0][n] = n;
    for (let n = 1; n <= t.length; n++)
        for (let o = 1; o <= e.length; o++) {
            let i;
            if (e[o - 1] === t[n - 1]) i = 0;
            else i = 1;
            if (
                ((r[o][n] = Math.min(
                    r[o - 1][n] + 1,
                    r[o][n - 1] + 1,
                    r[o - 1][n - 1] + i,
                )),
                o > 1 &&
                    n > 1 &&
                    e[o - 1] === t[n - 2] &&
                    e[o - 2] === t[n - 1])
            )
                r[o][n] = Math.min(r[o][n], r[o - 2][n - 2] + 1);
        }
    return r[e.length][t.length];
}
function ht(e, t) {
    if (!t || t.length === 0) return "";
    t = Array.from(new Set(t));
    let r = e.startsWith("--");
    if (r) ((e = e.slice(2)), (t = t.map((s) => s.slice(2))));
    let n = [],
        o = 3,
        i = 0.4;
    if (
        (t.forEach((s) => {
            if (s.length <= 1) return;
            let c = Xs(e, s),
                a = Math.max(e.length, s.length);
            if ((a - c) / a > i) {
                if (c < o) ((o = c), (n = [s]));
                else if (c === o) n.push(s);
            }
        }),
        n.sort((s, c) => s.localeCompare(c)),
        r)
    )
        n = n.map((s) => `--${s}`);
    if (n.length > 1)
        return `
(Did you mean one of ${n.join(", ")}?)`;
    if (n.length === 1)
        return `
(Did you mean ${n[0]}?)`;
    return "";
}
class ie extends Ys {
    constructor(e) {
        super();
        ((this.commands = []),
            (this.options = []),
            (this.parent = null),
            (this._allowUnknownOption = !1),
            (this._allowExcessArguments = !1),
            (this.registeredArguments = []),
            (this._args = this.registeredArguments),
            (this.args = []),
            (this.rawArgs = []),
            (this.processedArgs = []),
            (this._scriptPath = null),
            (this._name = e || ""),
            (this._optionValues = {}),
            (this._optionValueSources = {}),
            (this._storeOptionsAsProperties = !1),
            (this._actionHandler = null),
            (this._executableHandler = !1),
            (this._executableFile = null),
            (this._executableDir = null),
            (this._defaultCommandName = null),
            (this._exitCallback = null),
            (this._aliases = []),
            (this._combineFlagAndOptionalValue = !0),
            (this._description = ""),
            (this._summary = ""),
            (this._argsDescription = void 0),
            (this._enablePositionalOptions = !1),
            (this._passThroughOptions = !1),
            (this._lifeCycleHooks = {}),
            (this._showHelpAfterError = !1),
            (this._showSuggestionAfterError = !0),
            (this._savedState = null),
            (this._outputConfiguration = {
                writeOut: (t) => b.stdout.write(t),
                writeErr: (t) => b.stderr.write(t),
                outputError: (t, r) => r(t),
                getOutHelpWidth: () =>
                    b.stdout.isTTY ? b.stdout.columns : void 0,
                getErrHelpWidth: () =>
                    b.stderr.isTTY ? b.stderr.columns : void 0,
                getOutHasColors: () =>
                    vr() ?? (b.stdout.isTTY && b.stdout.hasColors?.()),
                getErrHasColors: () =>
                    vr() ?? (b.stderr.isTTY && b.stderr.hasColors?.()),
                stripColor: (t) => Qs(t),
            }),
            (this._hidden = !1),
            (this._helpOption = void 0),
            (this._addImplicitHelpCommand = void 0),
            (this._helpCommand = void 0),
            (this._helpConfiguration = {}),
            (this._helpGroupHeading = void 0),
            (this._defaultCommandGroup = void 0),
            (this._defaultOptionGroup = void 0));
    }
    copyInheritedSettings(e) {
        return (
            (this._outputConfiguration = e._outputConfiguration),
            (this._helpOption = e._helpOption),
            (this._helpCommand = e._helpCommand),
            (this._helpConfiguration = e._helpConfiguration),
            (this._exitCallback = e._exitCallback),
            (this._storeOptionsAsProperties = e._storeOptionsAsProperties),
            (this._combineFlagAndOptionalValue =
                e._combineFlagAndOptionalValue),
            (this._allowExcessArguments = e._allowExcessArguments),
            (this._enablePositionalOptions = e._enablePositionalOptions),
            (this._showHelpAfterError = e._showHelpAfterError),
            (this._showSuggestionAfterError = e._showSuggestionAfterError),
            this
        );
    }
    _getCommandAndAncestors() {
        let e = [];
        for (let t = this; t; t = t.parent) e.push(t);
        return e;
    }
    command(e, t, r) {
        let n = t,
            o = r;
        if (typeof n === "object" && n !== null) ((o = n), (n = null));
        o = o || {};
        let [, i, s] = e.match(/([^ ]+) *(.*)/),
            c = this.createCommand(i);
        if (n) (c.description(n), (c._executableHandler = !0));
        if (o.isDefault) this._defaultCommandName = c._name;
        if (
            ((c._hidden = !!(o.noHelp || o.hidden)),
            (c._executableFile = o.executableFile || null),
            s)
        )
            c.arguments(s);
        if (
            (this._registerCommand(c),
            (c.parent = this),
            c.copyInheritedSettings(this),
            n)
        )
            return this;
        return c;
    }
    createCommand(e) {
        return new ie(e);
    }
    createHelp() {
        return Object.assign(new Ae(), this.configureHelp());
    }
    configureHelp(e) {
        if (e === void 0) return this._helpConfiguration;
        return ((this._helpConfiguration = e), this);
    }
    configureOutput(e) {
        if (e === void 0) return this._outputConfiguration;
        return (
            (this._outputConfiguration = {
                ...this._outputConfiguration,
                ...e,
            }),
            this
        );
    }
    showHelpAfterError(e = !0) {
        if (typeof e !== "string") e = !!e;
        return ((this._showHelpAfterError = e), this);
    }
    showSuggestionAfterError(e = !0) {
        return ((this._showSuggestionAfterError = !!e), this);
    }
    addCommand(e, t) {
        if (!e._name)
            throw Error(`Command passed to .addCommand() must have a name
- specify the name in Command constructor or using .name()`);
        if (((t = t || {}), t.isDefault)) this._defaultCommandName = e._name;
        if (t.noHelp || t.hidden) e._hidden = !0;
        return (
            this._registerCommand(e),
            (e.parent = this),
            e._checkForBrokenPassThrough(),
            this
        );
    }
    createArgument(e, t) {
        return new Pe(e, t);
    }
    argument(e, t, r, n) {
        let o = this.createArgument(e, t);
        if (typeof r === "function") o.default(n).argParser(r);
        else o.default(r);
        return (this.addArgument(o), this);
    }
    arguments(e) {
        return (
            e
                .trim()
                .split(/ +/)
                .forEach((t) => {
                    this.argument(t);
                }),
            this
        );
    }
    addArgument(e) {
        let t = this.registeredArguments.slice(-1)[0];
        if (t?.variadic)
            throw Error(`only the last argument can be variadic '${t.name()}'`);
        if (e.required && e.defaultValue !== void 0 && e.parseArg === void 0)
            throw Error(
                `a default value for a required argument is never used: '${e.name()}'`,
            );
        return (this.registeredArguments.push(e), this);
    }
    helpCommand(e, t) {
        if (typeof e === "boolean") {
            if (
                ((this._addImplicitHelpCommand = e),
                e && this._defaultCommandGroup)
            )
                this._initCommandGroup(this._getHelpCommand());
            return this;
        }
        let r = e ?? "help [command]",
            [, n, o] = r.match(/([^ ]+) *(.*)/),
            i = t ?? "display help for command",
            s = this.createCommand(n);
        if ((s.helpOption(!1), o)) s.arguments(o);
        if (i) s.description(i);
        if (
            ((this._addImplicitHelpCommand = !0),
            (this._helpCommand = s),
            e || t)
        )
            this._initCommandGroup(s);
        return this;
    }
    addHelpCommand(e, t) {
        if (typeof e !== "object") return (this.helpCommand(e, t), this);
        return (
            (this._addImplicitHelpCommand = !0),
            (this._helpCommand = e),
            this._initCommandGroup(e),
            this
        );
    }
    _getHelpCommand() {
        if (
            this._addImplicitHelpCommand ??
            (this.commands.length &&
                !this._actionHandler &&
                !this._findCommand("help"))
        ) {
            if (this._helpCommand === void 0) this.helpCommand(void 0, void 0);
            return this._helpCommand;
        }
        return null;
    }
    hook(e, t) {
        let r = ["preSubcommand", "preAction", "postAction"];
        if (!r.includes(e))
            throw Error(`Unexpected value for event passed to hook : '${e}'.
Expecting one of '${r.join("', '")}'`);
        if (this._lifeCycleHooks[e]) this._lifeCycleHooks[e].push(t);
        else this._lifeCycleHooks[e] = [t];
        return this;
    }
    exitOverride(e) {
        if (e) this._exitCallback = e;
        else
            this._exitCallback = (t) => {
                if (t.code !== "commander.executeSubCommandAsync") throw t;
            };
        return this;
    }
    _exit(e, t, r) {
        if (this._exitCallback) this._exitCallback(new ee(e, t, r));
        b.exit(e);
    }
    action(e) {
        let t = (r) => {
            let n = this.registeredArguments.length,
                o = r.slice(0, n);
            if (this._storeOptionsAsProperties) o[n] = this;
            else o[n] = this.opts();
            return (o.push(this), e.apply(this, o));
        };
        return ((this._actionHandler = t), this);
    }
    createOption(e, t) {
        return new de(e, t);
    }
    _callParseArg(e, t, r, n) {
        try {
            return e.parseArg(t, r);
        } catch (o) {
            if (o.code === "commander.invalidArgument") {
                let i = `${n} ${o.message}`;
                this.error(i, { exitCode: o.exitCode, code: o.code });
            }
            throw o;
        }
    }
    _registerOption(e) {
        let t =
            (e.short && this._findOption(e.short)) ||
            (e.long && this._findOption(e.long));
        if (t) {
            let r = e.long && this._findOption(e.long) ? e.long : e.short;
            throw Error(`Cannot add option '${e.flags}'${this._name && ` to command '${this._name}'`} due to conflicting flag '${r}'
-  already used by option '${t.flags}'`);
        }
        (this._initOptionGroup(e), this.options.push(e));
    }
    _registerCommand(e) {
        let t = (n) => [n.name()].concat(n.aliases()),
            r = t(e).find((n) => this._findCommand(n));
        if (r) {
            let n = t(this._findCommand(r)).join("|"),
                o = t(e).join("|");
            throw Error(
                `cannot add command '${o}' as already have command '${n}'`,
            );
        }
        (this._initCommandGroup(e), this.commands.push(e));
    }
    addOption(e) {
        this._registerOption(e);
        let t = e.name(),
            r = e.attributeName();
        if (e.defaultValue !== void 0)
            this.setOptionValueWithSource(r, e.defaultValue, "default");
        let n = (o, i, s) => {
            if (o == null && e.presetArg !== void 0) o = e.presetArg;
            let c = this.getOptionValue(r);
            if (o !== null && e.parseArg) o = this._callParseArg(e, o, c, i);
            else if (o !== null && e.variadic) o = e._collectValue(o, c);
            if (o == null)
                if (e.negate) o = !1;
                else if (e.isBoolean() || e.optional) o = !0;
                else o = "";
            this.setOptionValueWithSource(r, o, s);
        };
        if (
            (this.on("option:" + t, (o) => {
                let i = `error: option '${e.flags}' argument '${o}' is invalid.`;
                n(o, i, "cli");
            }),
            e.envVar)
        )
            this.on("optionEnv:" + t, (o) => {
                let i = `error: option '${e.flags}' value '${o}' from env '${e.envVar}' is invalid.`;
                n(o, i, "env");
            });
        return this;
    }
    _optionEx(e, t, r, n, o) {
        if (typeof t === "object" && t instanceof de)
            throw Error(
                "To add an Option object use addOption() instead of option() or requiredOption()",
            );
        let i = this.createOption(t, r);
        if ((i.makeOptionMandatory(!!e.mandatory), typeof n === "function"))
            i.default(o).argParser(n);
        else if (n instanceof RegExp) {
            let s = n;
            ((n = (c, a) => {
                let u = s.exec(c);
                return u ? u[0] : a;
            }),
                i.default(o).argParser(n));
        } else i.default(n);
        return this.addOption(i);
    }
    option(e, t, r, n) {
        return this._optionEx({}, e, t, r, n);
    }
    requiredOption(e, t, r, n) {
        return this._optionEx({ mandatory: !0 }, e, t, r, n);
    }
    combineFlagAndOptionalValue(e = !0) {
        return ((this._combineFlagAndOptionalValue = !!e), this);
    }
    allowUnknownOption(e = !0) {
        return ((this._allowUnknownOption = !!e), this);
    }
    allowExcessArguments(e = !0) {
        return ((this._allowExcessArguments = !!e), this);
    }
    enablePositionalOptions(e = !0) {
        return ((this._enablePositionalOptions = !!e), this);
    }
    passThroughOptions(e = !0) {
        return (
            (this._passThroughOptions = !!e),
            this._checkForBrokenPassThrough(),
            this
        );
    }
    _checkForBrokenPassThrough() {
        if (
            this.parent &&
            this._passThroughOptions &&
            !this.parent._enablePositionalOptions
        )
            throw Error(
                `passThroughOptions cannot be used for '${this._name}' without turning on enablePositionalOptions for parent command(s)`,
            );
    }
    storeOptionsAsProperties(e = !0) {
        if (this.options.length)
            throw Error(
                "call .storeOptionsAsProperties() before adding options",
            );
        if (Object.keys(this._optionValues).length)
            throw Error(
                "call .storeOptionsAsProperties() before setting option values",
            );
        return ((this._storeOptionsAsProperties = !!e), this);
    }
    getOptionValue(e) {
        if (this._storeOptionsAsProperties) return this[e];
        return this._optionValues[e];
    }
    setOptionValue(e, t) {
        return this.setOptionValueWithSource(e, t, void 0);
    }
    setOptionValueWithSource(e, t, r) {
        if (this._storeOptionsAsProperties) this[e] = t;
        else this._optionValues[e] = t;
        return ((this._optionValueSources[e] = r), this);
    }
    getOptionValueSource(e) {
        return this._optionValueSources[e];
    }
    getOptionValueSourceWithGlobals(e) {
        let t;
        return (
            this._getCommandAndAncestors().forEach((r) => {
                if (Object.hasOwn(r._optionValueSources, e))
                    t = r.getOptionValueSource(e);
            }),
            t
        );
    }
    _prepareUserArgs(e, t) {
        if (e !== void 0 && !Array.isArray(e))
            throw Error("first parameter to parse must be array or undefined");
        if (((t = t || {}), e === void 0 && t.from === void 0)) {
            if (b.versions?.electron) t.from = "electron";
            let n = b.execArgv ?? [];
            if (
                n.includes("-e") ||
                n.includes("--eval") ||
                n.includes("-p") ||
                n.includes("--print")
            )
                t.from = "eval";
        }
        if (e === void 0) e = b.argv;
        this.rawArgs = e.slice();
        let r;
        switch (t.from) {
            case void 0:
            case "node":
                ((this._scriptPath = e[1]), (r = e.slice(2)));
                break;
            case "electron":
                if (b.defaultApp) ((this._scriptPath = e[1]), (r = e.slice(2)));
                else r = e.slice(1);
                break;
            case "user":
                r = e.slice(0);
                break;
            case "eval":
                r = e.slice(1);
                break;
            default:
                throw Error(`unexpected parse option { from: '${t.from}' }`);
        }
        if (!this._name && this._scriptPath)
            this.nameFromFilename(this._scriptPath);
        return ((this._name = this._name || "program"), r);
    }
    parse(e, t) {
        this._prepareForParse();
        let r = this._prepareUserArgs(e, t);
        return (this._parseCommand([], r), this);
    }
    async parseAsync(e, t) {
        this._prepareForParse();
        let r = this._prepareUserArgs(e, t);
        return (await this._parseCommand([], r), this);
    }
    _prepareForParse() {
        if (this._savedState === null)
            (this.options
                .filter(
                    (e) =>
                        e.negate &&
                        e.defaultValue === void 0 &&
                        this.getOptionValue(e.attributeName()) === void 0,
                )
                .forEach((e) => {
                    let t = e.long.replace(/^--no-/, "--");
                    if (!this._findOption(t))
                        this.setOptionValueWithSource(
                            e.attributeName(),
                            !0,
                            "default",
                        );
                }),
                this.saveStateBeforeParse());
        else this.restoreStateBeforeParse();
    }
    saveStateBeforeParse() {
        this._savedState = {
            _name: this._name,
            _optionValues: { ...this._optionValues },
            _optionValueSources: { ...this._optionValueSources },
        };
    }
    restoreStateBeforeParse() {
        if (this._storeOptionsAsProperties)
            throw Error(`Can not call parse again when storeOptionsAsProperties is true.
- either make a new Command for each call to parse, or stop storing options as properties`);
        ((this._name = this._savedState._name),
            (this._scriptPath = null),
            (this.rawArgs = []),
            (this._optionValues = { ...this._savedState._optionValues }),
            (this._optionValueSources = {
                ...this._savedState._optionValueSources,
            }),
            (this.args = []),
            (this.processedArgs = []));
    }
    _checkForMissingExecutable(e, t, r) {
        if (Ce.existsSync(e)) return;
        let n = t
                ? `searched for local subcommand relative to directory '${t}'`
                : "no directory for search for local subcommand, use .executableDir() to supply a custom directory",
            o = `'${e}' does not exist
 - if '${r}' is not meant to be an executable command, remove description parameter from '.command()' and use '.description()' instead
 - if the default executable name is not suitable, use the executableFile option to supply a custom name or path
 - ${n}`;
        throw Error(o);
    }
    _executeSubCommand(e, t) {
        t = t.slice();
        let r = [".js", ".ts", ".tsx", ".mjs", ".cjs"];
        function n(u, l) {
            let f = B.resolve(u, l);
            if (Ce.existsSync(f)) return f;
            if (r.includes(B.extname(l))) return;
            let d = r.find((h) => Ce.existsSync(`${f}${h}`));
            if (d) return `${f}${d}`;
            return;
        }
        (this._checkForMissingMandatoryOptions(),
            this._checkForConflictingOptions());
        let o = e._executableFile || `${this._name}-${e._name}`,
            i = this._executableDir || "";
        if (this._scriptPath) {
            let u;
            try {
                u = Ce.realpathSync(this._scriptPath);
            } catch {
                u = this._scriptPath;
            }
            i = B.resolve(B.dirname(u), i);
        }
        if (i) {
            let u = n(i, o);
            if (!u && !e._executableFile && this._scriptPath) {
                let l = B.basename(
                    this._scriptPath,
                    B.extname(this._scriptPath),
                );
                if (l !== this._name) u = n(i, `${l}-${e._name}`);
            }
            o = u || o;
        }
        let s = r.includes(B.extname(o)),
            c;
        if (b.platform !== "win32")
            if (s)
                (t.unshift(o),
                    (t = br(b.execArgv).concat(t)),
                    (c = mt.spawn(b.argv[0], t, { stdio: "inherit" })));
            else c = mt.spawn(o, t, { stdio: "inherit" });
        else
            (this._checkForMissingExecutable(o, i, e._name),
                t.unshift(o),
                (t = br(b.execArgv).concat(t)),
                (c = mt.spawn(b.execPath, t, { stdio: "inherit" })));
        if (!c.killed)
            ["SIGUSR1", "SIGUSR2", "SIGTERM", "SIGINT", "SIGHUP"].forEach(
                (l) => {
                    b.on(l, () => {
                        if (c.killed === !1 && c.exitCode === null) c.kill(l);
                    });
                },
            );
        let a = this._exitCallback;
        (c.on("close", (u) => {
            if (((u = u ?? 1), !a)) b.exit(u);
            else a(new ee(u, "commander.executeSubCommandAsync", "(close)"));
        }),
            c.on("error", (u) => {
                if (u.code === "ENOENT")
                    this._checkForMissingExecutable(o, i, e._name);
                else if (u.code === "EACCES")
                    throw Error(`'${o}' not executable`);
                if (!a) b.exit(1);
                else {
                    let l = new ee(
                        1,
                        "commander.executeSubCommandAsync",
                        "(error)",
                    );
                    ((l.nestedError = u), a(l));
                }
            }),
            (this.runningCommand = c));
    }
    _dispatchSubcommand(e, t, r) {
        let n = this._findCommand(e);
        if (!n) this.help({ error: !0 });
        n._prepareForParse();
        let o;
        return (
            (o = this._chainOrCallSubCommandHook(o, n, "preSubcommand")),
            (o = this._chainOrCall(o, () => {
                if (n._executableHandler)
                    this._executeSubCommand(n, t.concat(r));
                else return n._parseCommand(t, r);
            })),
            o
        );
    }
    _dispatchHelpCommand(e) {
        if (!e) this.help();
        let t = this._findCommand(e);
        if (t && !t._executableHandler) t.help();
        return this._dispatchSubcommand(
            e,
            [],
            [
                this._getHelpOption()?.long ??
                    this._getHelpOption()?.short ??
                    "--help",
            ],
        );
    }
    _checkNumberOfArguments() {
        if (
            (this.registeredArguments.forEach((e, t) => {
                if (e.required && this.args[t] == null)
                    this.missingArgument(e.name());
            }),
            this.registeredArguments.length > 0 &&
                this.registeredArguments[this.registeredArguments.length - 1]
                    .variadic)
        )
            return;
        if (this.args.length > this.registeredArguments.length)
            this._excessArguments(this.args);
    }
    _processArguments() {
        let e = (r, n, o) => {
            let i = n;
            if (n !== null && r.parseArg) {
                let s = `error: command-argument value '${n}' is invalid for argument '${r.name()}'.`;
                i = this._callParseArg(r, n, o, s);
            }
            return i;
        };
        this._checkNumberOfArguments();
        let t = [];
        (this.registeredArguments.forEach((r, n) => {
            let o = r.defaultValue;
            if (r.variadic) {
                if (n < this.args.length) {
                    if (((o = this.args.slice(n)), r.parseArg))
                        o = o.reduce((i, s) => e(r, s, i), r.defaultValue);
                } else if (o === void 0) o = [];
            } else if (n < this.args.length) {
                if (((o = this.args[n]), r.parseArg))
                    o = e(r, o, r.defaultValue);
            }
            t[n] = o;
        }),
            (this.processedArgs = t));
    }
    _chainOrCall(e, t) {
        if (e?.then && typeof e.then === "function") return e.then(() => t());
        return t();
    }
    _chainOrCallHooks(e, t) {
        let r = e,
            n = [];
        if (
            (this._getCommandAndAncestors()
                .reverse()
                .filter((o) => o._lifeCycleHooks[t] !== void 0)
                .forEach((o) => {
                    o._lifeCycleHooks[t].forEach((i) => {
                        n.push({ hookedCommand: o, callback: i });
                    });
                }),
            t === "postAction")
        )
            n.reverse();
        return (
            n.forEach((o) => {
                r = this._chainOrCall(r, () =>
                    o.callback(o.hookedCommand, this),
                );
            }),
            r
        );
    }
    _chainOrCallSubCommandHook(e, t, r) {
        let n = e;
        if (this._lifeCycleHooks[r] !== void 0)
            this._lifeCycleHooks[r].forEach((o) => {
                n = this._chainOrCall(n, () => o(this, t));
            });
        return n;
    }
    _parseCommand(e, t) {
        let r = this.parseOptions(t);
        if (
            (this._parseOptionsEnv(),
            this._parseOptionsImplied(),
            (e = e.concat(r.operands)),
            (t = r.unknown),
            (this.args = e.concat(t)),
            e && this._findCommand(e[0]))
        )
            return this._dispatchSubcommand(e[0], e.slice(1), t);
        if (this._getHelpCommand() && e[0] === this._getHelpCommand().name())
            return this._dispatchHelpCommand(e[1]);
        if (this._defaultCommandName)
            return (
                this._outputHelpIfRequested(t),
                this._dispatchSubcommand(this._defaultCommandName, e, t)
            );
        if (
            this.commands.length &&
            this.args.length === 0 &&
            !this._actionHandler &&
            !this._defaultCommandName
        )
            this.help({ error: !0 });
        (this._outputHelpIfRequested(r.unknown),
            this._checkForMissingMandatoryOptions(),
            this._checkForConflictingOptions());
        let n = () => {
                if (r.unknown.length > 0) this.unknownOption(r.unknown[0]);
            },
            o = `command:${this.name()}`;
        if (this._actionHandler) {
            (n(), this._processArguments());
            let i;
            if (
                ((i = this._chainOrCallHooks(i, "preAction")),
                (i = this._chainOrCall(i, () =>
                    this._actionHandler(this.processedArgs),
                )),
                this.parent)
            )
                i = this._chainOrCall(i, () => {
                    this.parent.emit(o, e, t);
                });
            return ((i = this._chainOrCallHooks(i, "postAction")), i);
        }
        if (this.parent?.listenerCount(o))
            (n(), this._processArguments(), this.parent.emit(o, e, t));
        else if (e.length) {
            if (this._findCommand("*"))
                return this._dispatchSubcommand("*", e, t);
            if (this.listenerCount("command:*")) this.emit("command:*", e, t);
            else if (this.commands.length) this.unknownCommand();
            else (n(), this._processArguments());
        } else if (this.commands.length) (n(), this.help({ error: !0 }));
        else (n(), this._processArguments());
    }
    _findCommand(e) {
        if (!e) return;
        return this.commands.find(
            (t) => t._name === e || t._aliases.includes(e),
        );
    }
    _findOption(e) {
        return this.options.find((t) => t.is(e));
    }
    _checkForMissingMandatoryOptions() {
        this._getCommandAndAncestors().forEach((e) => {
            e.options.forEach((t) => {
                if (
                    t.mandatory &&
                    e.getOptionValue(t.attributeName()) === void 0
                )
                    e.missingMandatoryOptionValue(t);
            });
        });
    }
    _checkForConflictingLocalOptions() {
        let e = this.options.filter((r) => {
            let n = r.attributeName();
            if (this.getOptionValue(n) === void 0) return !1;
            return this.getOptionValueSource(n) !== "default";
        });
        e.filter((r) => r.conflictsWith.length > 0).forEach((r) => {
            let n = e.find((o) => r.conflictsWith.includes(o.attributeName()));
            if (n) this._conflictingOption(r, n);
        });
    }
    _checkForConflictingOptions() {
        this._getCommandAndAncestors().forEach((e) => {
            e._checkForConflictingLocalOptions();
        });
    }
    parseOptions(e) {
        let t = [],
            r = [],
            n = t;
        function o(u) {
            return u.length > 1 && u[0] === "-";
        }
        let i = (u) => {
                if (!/^-(\d+|\d*\.\d+)([eE][+-]?\d+)?$/.test(u)) return !1;
                return !this._getCommandAndAncestors().some((l) =>
                    l.options.map((f) => f.short).some((f) => /^-\d$/.test(f)),
                );
            },
            s = null,
            c = null,
            a = 0;
        while (a < e.length || c) {
            let u = c ?? e[a++];
            if (((c = null), u === "--")) {
                if (n === r) n.push(u);
                n.push(...e.slice(a));
                break;
            }
            if (s && (!o(u) || i(u))) {
                this.emit(`option:${s.name()}`, u);
                continue;
            }
            if (((s = null), o(u))) {
                let l = this._findOption(u);
                if (l) {
                    if (l.required) {
                        let f = e[a++];
                        if (f === void 0) this.optionMissingArgument(l);
                        this.emit(`option:${l.name()}`, f);
                    } else if (l.optional) {
                        let f = null;
                        if (a < e.length && (!o(e[a]) || i(e[a]))) f = e[a++];
                        this.emit(`option:${l.name()}`, f);
                    } else this.emit(`option:${l.name()}`);
                    s = l.variadic ? l : null;
                    continue;
                }
            }
            if (u.length > 2 && u[0] === "-" && u[1] !== "-") {
                let l = this._findOption(`-${u[1]}`);
                if (l) {
                    if (
                        l.required ||
                        (l.optional && this._combineFlagAndOptionalValue)
                    )
                        this.emit(`option:${l.name()}`, u.slice(2));
                    else
                        (this.emit(`option:${l.name()}`),
                            (c = `-${u.slice(2)}`));
                    continue;
                }
            }
            if (/^--[^=]+=/.test(u)) {
                let l = u.indexOf("="),
                    f = this._findOption(u.slice(0, l));
                if (f && (f.required || f.optional)) {
                    this.emit(`option:${f.name()}`, u.slice(l + 1));
                    continue;
                }
            }
            if (n === t && o(u) && !(this.commands.length === 0 && i(u))) n = r;
            if (
                (this._enablePositionalOptions || this._passThroughOptions) &&
                t.length === 0 &&
                r.length === 0
            ) {
                if (this._findCommand(u)) {
                    (t.push(u), r.push(...e.slice(a)));
                    break;
                } else if (
                    this._getHelpCommand() &&
                    u === this._getHelpCommand().name()
                ) {
                    t.push(u, ...e.slice(a));
                    break;
                } else if (this._defaultCommandName) {
                    r.push(u, ...e.slice(a));
                    break;
                }
            }
            if (this._passThroughOptions) {
                n.push(u, ...e.slice(a));
                break;
            }
            n.push(u);
        }
        return { operands: t, unknown: r };
    }
    opts() {
        if (this._storeOptionsAsProperties) {
            let e = {},
                t = this.options.length;
            for (let r = 0; r < t; r++) {
                let n = this.options[r].attributeName();
                e[n] = n === this._versionOptionName ? this._version : this[n];
            }
            return e;
        }
        return this._optionValues;
    }
    optsWithGlobals() {
        return this._getCommandAndAncestors().reduce(
            (e, t) => Object.assign(e, t.opts()),
            {},
        );
    }
    error(e, t) {
        if (
            (this._outputConfiguration.outputError(
                `${e}
`,
                this._outputConfiguration.writeErr,
            ),
            typeof this._showHelpAfterError === "string")
        )
            this._outputConfiguration.writeErr(`${this._showHelpAfterError}
`);
        else if (this._showHelpAfterError)
            (this._outputConfiguration.writeErr(`
`),
                this.outputHelp({ error: !0 }));
        let r = t || {},
            n = r.exitCode || 1,
            o = r.code || "commander.error";
        this._exit(n, o, e);
    }
    _parseOptionsEnv() {
        this.options.forEach((e) => {
            if (e.envVar && e.envVar in b.env) {
                let t = e.attributeName();
                if (
                    this.getOptionValue(t) === void 0 ||
                    ["default", "config", "env"].includes(
                        this.getOptionValueSource(t),
                    )
                )
                    if (e.required || e.optional)
                        this.emit(`optionEnv:${e.name()}`, b.env[e.envVar]);
                    else this.emit(`optionEnv:${e.name()}`);
            }
        });
    }
    _parseOptionsImplied() {
        let e = new dt(this.options),
            t = (r) =>
                this.getOptionValue(r) !== void 0 &&
                !["default", "implied"].includes(this.getOptionValueSource(r));
        this.options
            .filter(
                (r) =>
                    r.implied !== void 0 &&
                    t(r.attributeName()) &&
                    e.valueFromOption(
                        this.getOptionValue(r.attributeName()),
                        r,
                    ),
            )
            .forEach((r) => {
                Object.keys(r.implied)
                    .filter((n) => !t(n))
                    .forEach((n) => {
                        this.setOptionValueWithSource(
                            n,
                            r.implied[n],
                            "implied",
                        );
                    });
            });
    }
    missingArgument(e) {
        let t = `error: missing required argument '${e}'`;
        this.error(t, { code: "commander.missingArgument" });
    }
    optionMissingArgument(e) {
        let t = `error: option '${e.flags}' argument missing`;
        this.error(t, { code: "commander.optionMissingArgument" });
    }
    missingMandatoryOptionValue(e) {
        let t = `error: required option '${e.flags}' not specified`;
        this.error(t, { code: "commander.missingMandatoryOptionValue" });
    }
    _conflictingOption(e, t) {
        let r = (i) => {
                let s = i.attributeName(),
                    c = this.getOptionValue(s),
                    a = this.options.find(
                        (l) => l.negate && s === l.attributeName(),
                    ),
                    u = this.options.find(
                        (l) => !l.negate && s === l.attributeName(),
                    );
                if (
                    a &&
                    ((a.presetArg === void 0 && c === !1) ||
                        (a.presetArg !== void 0 && c === a.presetArg))
                )
                    return a;
                return u || i;
            },
            n = (i) => {
                let s = r(i),
                    c = s.attributeName();
                if (this.getOptionValueSource(c) === "env")
                    return `environment variable '${s.envVar}'`;
                return `option '${s.flags}'`;
            },
            o = `error: ${n(e)} cannot be used with ${n(t)}`;
        this.error(o, { code: "commander.conflictingOption" });
    }
    unknownOption(e) {
        if (this._allowUnknownOption) return;
        let t = "";
        if (e.startsWith("--") && this._showSuggestionAfterError) {
            let n = [],
                o = this;
            do {
                let i = o
                    .createHelp()
                    .visibleOptions(o)
                    .filter((s) => s.long)
                    .map((s) => s.long);
                ((n = n.concat(i)), (o = o.parent));
            } while (o && !o._enablePositionalOptions);
            t = ht(e, n);
        }
        let r = `error: unknown option '${e}'${t}`;
        this.error(r, { code: "commander.unknownOption" });
    }
    _excessArguments(e) {
        if (this._allowExcessArguments) return;
        let t = this.registeredArguments.length,
            r = t === 1 ? "" : "s",
            n = e.length,
            o = this.parent ? ` for '${this.name()}'` : "",
            i = e.join(", "),
            s = `error: too many arguments${o}. Expected ${t} argument${r} but got ${n}: ${i}.`;
        this.error(s, { code: "commander.excessArguments" });
    }
    unknownCommand() {
        let e = this.args[0],
            t = "";
        if (this._showSuggestionAfterError) {
            let n = [];
            (this.createHelp()
                .visibleCommands(this)
                .forEach((o) => {
                    if ((n.push(o.name()), o.alias())) n.push(o.alias());
                }),
                (t = ht(e, n)));
        }
        let r = `error: unknown command '${e}'${t}`;
        this.error(r, { code: "commander.unknownCommand" });
    }
    version(e, t, r) {
        if (e === void 0) return this._version;
        ((this._version = e),
            (t = t || "-V, --version"),
            (r = r || "output the version number"));
        let n = this.createOption(t, r);
        return (
            (this._versionOptionName = n.attributeName()),
            this._registerOption(n),
            this.on("option:" + n.name(), () => {
                (this._outputConfiguration.writeOut(`${e}
`),
                    this._exit(0, "commander.version", e));
            }),
            this
        );
    }
    description(e, t) {
        if (e === void 0 && t === void 0) return this._description;
        if (((this._description = e), t)) this._argsDescription = t;
        return this;
    }
    summary(e) {
        if (e === void 0) return this._summary;
        return ((this._summary = e), this);
    }
    alias(e) {
        if (e === void 0) return this._aliases[0];
        let t = this;
        if (
            this.commands.length !== 0 &&
            this.commands[this.commands.length - 1]._executableHandler
        )
            t = this.commands[this.commands.length - 1];
        if (e === t._name)
            throw Error("Command alias can't be the same as its name");
        let r = this.parent?._findCommand(e);
        if (r) {
            let n = [r.name()].concat(r.aliases()).join("|");
            throw Error(
                `cannot add alias '${e}' to command '${this.name()}' as already have command '${n}'`,
            );
        }
        return (t._aliases.push(e), this);
    }
    aliases(e) {
        if (e === void 0) return this._aliases;
        return (e.forEach((t) => this.alias(t)), this);
    }
    usage(e) {
        if (e === void 0) {
            if (this._usage) return this._usage;
            let t = this.registeredArguments.map((r) => Ze(r));
            return []
                .concat(
                    this.options.length || this._helpOption !== null
                        ? "[options]"
                        : [],
                    this.commands.length ? "[command]" : [],
                    this.registeredArguments.length ? t : [],
                )
                .join(" ");
        }
        return ((this._usage = e), this);
    }
    name(e) {
        if (e === void 0) return this._name;
        return ((this._name = e), this);
    }
    helpGroup(e) {
        if (e === void 0) return this._helpGroupHeading ?? "";
        return ((this._helpGroupHeading = e), this);
    }
    commandsGroup(e) {
        if (e === void 0) return this._defaultCommandGroup ?? "";
        return ((this._defaultCommandGroup = e), this);
    }
    optionsGroup(e) {
        if (e === void 0) return this._defaultOptionGroup ?? "";
        return ((this._defaultOptionGroup = e), this);
    }
    _initOptionGroup(e) {
        if (this._defaultOptionGroup && !e.helpGroupHeading)
            e.helpGroup(this._defaultOptionGroup);
    }
    _initCommandGroup(e) {
        if (this._defaultCommandGroup && !e.helpGroup())
            e.helpGroup(this._defaultCommandGroup);
    }
    nameFromFilename(e) {
        return ((this._name = B.basename(e, B.extname(e))), this);
    }
    executableDir(e) {
        if (e === void 0) return this._executableDir;
        return ((this._executableDir = e), this);
    }
    helpInformation(e) {
        let t = this.createHelp(),
            r = this._getOutputContext(e);
        t.prepareContext({
            error: r.error,
            helpWidth: r.helpWidth,
            outputHasColors: r.hasColors,
        });
        let n = t.formatHelp(this, t);
        if (r.hasColors) return n;
        return this._outputConfiguration.stripColor(n);
    }
    _getOutputContext(e) {
        e = e || {};
        let t = !!e.error,
            r,
            n,
            o;
        if (t)
            ((r = (s) => this._outputConfiguration.writeErr(s)),
                (n = this._outputConfiguration.getErrHasColors()),
                (o = this._outputConfiguration.getErrHelpWidth()));
        else
            ((r = (s) => this._outputConfiguration.writeOut(s)),
                (n = this._outputConfiguration.getOutHasColors()),
                (o = this._outputConfiguration.getOutHelpWidth()));
        return {
            error: t,
            write: (s) => {
                if (!n) s = this._outputConfiguration.stripColor(s);
                return r(s);
            },
            hasColors: n,
            helpWidth: o,
        };
    }
    outputHelp(e) {
        let t;
        if (typeof e === "function") ((t = e), (e = void 0));
        let r = this._getOutputContext(e),
            n = { error: r.error, write: r.write, command: this };
        (this._getCommandAndAncestors()
            .reverse()
            .forEach((i) => i.emit("beforeAllHelp", n)),
            this.emit("beforeHelp", n));
        let o = this.helpInformation({ error: r.error });
        if (t) {
            if (((o = t(o)), typeof o !== "string" && !Buffer.isBuffer(o)))
                throw Error(
                    "outputHelp callback must return a string or a Buffer",
                );
        }
        if ((r.write(o), this._getHelpOption()?.long))
            this.emit(this._getHelpOption().long);
        (this.emit("afterHelp", n),
            this._getCommandAndAncestors().forEach((i) =>
                i.emit("afterAllHelp", n),
            ));
    }
    helpOption(e, t) {
        if (typeof e === "boolean") {
            if (e) {
                if (this._helpOption === null) this._helpOption = void 0;
                if (this._defaultOptionGroup)
                    this._initOptionGroup(this._getHelpOption());
            } else this._helpOption = null;
            return this;
        }
        if (
            ((this._helpOption = this.createOption(
                e ?? "-h, --help",
                t ?? "display help for command",
            )),
            e || t)
        )
            this._initOptionGroup(this._helpOption);
        return this;
    }
    _getHelpOption() {
        if (this._helpOption === void 0) this.helpOption(void 0, void 0);
        return this._helpOption;
    }
    addHelpOption(e) {
        return ((this._helpOption = e), this._initOptionGroup(e), this);
    }
    help(e) {
        this.outputHelp(e);
        let t = Number(b.exitCode ?? 0);
        if (t === 0 && e && typeof e !== "function" && e.error) t = 1;
        this._exit(t, "commander.help", "(outputHelp)");
    }
    addHelpText(e, t) {
        let r = ["beforeAll", "before", "after", "afterAll"];
        if (!r.includes(e))
            throw Error(`Unexpected value for position to addHelpText.
Expecting one of '${r.join("', '")}'`);
        let n = `${e}Help`;
        return (
            this.on(n, (o) => {
                let i;
                if (typeof t === "function")
                    i = t({ error: o.error, command: o.command });
                else i = t;
                if (i)
                    o.write(`${i}
`);
            }),
            this
        );
    }
    _outputHelpIfRequested(e) {
        let t = this._getHelpOption();
        if (t && e.find((n) => t.is(n)))
            (this.outputHelp(),
                this._exit(0, "commander.helpDisplayed", "(outputHelp)"));
    }
}
function br(e) {
    return e.map((t) => {
        if (!t.startsWith("--inspect")) return t;
        let r,
            n = "127.0.0.1",
            o = "9229",
            i;
        if ((i = t.match(/^(--inspect(-brk|-wait)?)$/)) !== null) r = i[1];
        else if (
            (i = t.match(
                /^(--inspect(-brk|-port|-wait)?)=(\[[^\]]+\]|[^:]+)$/,
            )) !== null
        )
            if (((r = i[1]), /^\d+$/.test(i[3]))) o = i[3];
            else n = i[3];
        else if (
            (i = t.match(
                /^(--inspect(-brk|-port|-wait)?)=(\[[^\]]+\]|[^:]+):(\d+)$/,
            )) !== null
        )
            ((r = i[1]), (n = i[3]), (o = i[4]));
        if (r && o !== "0") return `${r}=${n}:${parseInt(o) + 1}`;
        return t;
    });
}
function vr() {
    if (
        b.env.NO_COLOR ||
        b.env.FORCE_COLOR === "0" ||
        b.env.FORCE_COLOR === "false"
    )
        return !1;
    if (b.env.FORCE_COLOR || b.env.CLICOLOR_FORCE !== void 0) return !0;
    return;
}
var La = new ie();
function me(e) {
    let t = Object.values(e).filter((n) => typeof n === "number");
    return Object.entries(e)
        .filter(([n, o]) => t.indexOf(+n) === -1)
        .map(([n, o]) => o);
}
function _t(e, t = "|") {
    return e.map((r) => bt(r)).join(t);
}
function ge(e, t) {
    if (typeof t === "bigint") return t.toString();
    return t;
}
class $r {
    constructor(e) {
        ((this._getter = e), (this._value = void 0));
    }
    get value() {
        let e = this._getter;
        if (e !== void 0) ((this._value = e()), (this._getter = void 0));
        return this._value;
    }
}
function _e(e) {
    return new $r(e);
}
function zr(e) {
    return e === null || e === void 0;
}
function Ee(e) {
    let t = e.startsWith("^") ? 1 : 0,
        r = e.endsWith("$") ? e.length - 1 : e.length;
    return e.slice(t, r);
}
function wr(e, t) {
    let r = e / t,
        n = Math.round(r),
        o = 4 * Number.EPSILON * Math.max(Math.abs(r), 1);
    if (Math.abs(r - n) < o) return 0;
    return r - n;
}
function D(e, t, r) {
    Object.defineProperty(e, t, {
        value: r,
        writable: !0,
        enumerable: !0,
        configurable: !0,
    });
}
function ye(e) {
    let t = Object.getOwnPropertyDescriptor(e, "shape");
    return t?.get ? t.get.raw : t?.value;
}
function W(e) {
    return ye(e._zod.def) ?? e._zod.def.shape;
}
function kr(e, t, r) {
    Object.defineProperty(e, t, {
        get() {
            let n = r();
            return (D(this, t, n), n);
        },
        enumerable: !0,
        configurable: !0,
    });
}
function Or(e, t, r) {
    if (t in e) D(e, t, r);
    else e[t] = r;
}
function te(e, t, r, n) {
    let o = W(t);
    for (let i of r) {
        let s = Object.getOwnPropertyDescriptor(o, i);
        if (!s.enumerable) continue;
        if (s.get)
            kr(e, i, () => {
                let c = t._zod.def.shape[i];
                return n ? n(c, i) : c;
            });
        else Or(e, i, n ? n(s.value, i) : s.value);
    }
}
function ec(e, t) {
    for (let r of Reflect.ownKeys(t)) {
        let n = Object.getOwnPropertyDescriptor(t, r);
        if (!n.enumerable) continue;
        if (n.get) kr(e, r, () => t[r]);
        else Or(e, r, n.value);
    }
}
function E(...e) {
    let t = {};
    for (let r of e) {
        let n = Object.getOwnPropertyDescriptors(r);
        Object.assign(t, n);
    }
    return Object.defineProperties({}, t);
}
function Sr(e) {
    return JSON.stringify(e);
}
function Pr(e) {
    return e
        .toLowerCase()
        .trim()
        .replace(/[^\w\s-]/g, "")
        .replace(/[\s_-]+/g, "-")
        .replace(/^-+|-+$/g, "");
}
var yt = "captureStackTrace" in Error ? Error.captureStackTrace : (...e) => {};
function se(e) {
    return typeof e === "object" && e !== null && !Array.isArray(e);
}
var Zr = _e(() => {
    if (T.jitless) return !1;
    if (typeof navigator < "u" && navigator?.userAgent?.includes("Cloudflare"))
        return !1;
    try {
        return (new Function(""), !0);
    } catch (e) {
        return !1;
    }
});
function ce(e) {
    if (se(e) === !1) return !1;
    let t = e.constructor;
    if (t === void 0) return !0;
    if (typeof t !== "function") return !0;
    let r = t.prototype;
    if (se(r) === !1) return !1;
    if (Object.prototype.hasOwnProperty.call(r, "isPrototypeOf") === !1)
        return !1;
    return !0;
}
function xt(e) {
    if (ce(e)) return { ...e };
    if (Array.isArray(e)) return [...e];
    if (e instanceof Map) return new Map(e);
    if (e instanceof Set) return new Set(e);
    return e;
}
var Ar = new Set(["string", "number", "symbol"]);
function X(e) {
    return e.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
function U(e, t, r) {
    let n = new e._zod.constr(t ?? e._zod.def);
    if (!t || r?.parent) n._zod.parent = e;
    return n;
}
function g(e) {
    let t = e;
    if (!t) return {};
    if (typeof t === "string") return { error: () => t };
    if (t?.message !== void 0) {
        if (t?.error !== void 0)
            throw Error("Cannot specify both `message` and `error` params");
        t.error = t.message;
    }
    if ((delete t.message, typeof t.error === "string"))
        return { ...t, error: () => t.error };
    return t;
}
function bt(e) {
    if (typeof e === "bigint") return e.toString() + "n";
    if (typeof e === "string") return `"${e}"`;
    return `${e}`;
}
function Cr(e) {
    return Object.keys(e).filter(
        (t) => e[t]._zod.optin !== void 0 && e[t]._zod.optout === "optional",
    );
}
var Te = (() => ({
        safeint: [Number.MIN_SAFE_INTEGER, Number.MAX_SAFE_INTEGER],
        int32: [-2147483648, 2147483647],
        uint32: [0, 4294967295],
        float32: [
            -340282346638528860000000000000000000000,
            340282346638528860000000000000000000000,
        ],
        float64: [-Number.MAX_VALUE, Number.MAX_VALUE],
    }))(),
    vt = {
        int64: [BigInt("-9223372036854775808"), BigInt("9223372036854775807")],
        uint64: [BigInt(0), BigInt("18446744073709551615")],
    };
function tc(e, t) {
    let r = e._zod.def,
        n = r.checks;
    if (n && n.length > 0)
        throw Error(
            ".pick() cannot be used on object schemas containing refinements",
        );
    let i = {};
    return (te(i, e, Ie(e, t)), U(e, E(r, { shape: i, checks: [] })));
}
function Ie(e, t) {
    let r = W(e),
        n = [];
    for (let o of Reflect.ownKeys(t)) {
        if (!Object.getOwnPropertyDescriptor(r, o)?.enumerable)
            throw Error(`Unrecognized key: "${String(o)}"`);
        if (t[o]) n.push(o);
    }
    return n;
}
function rc(e, t) {
    let r = e._zod.def,
        n = r.checks;
    if (n && n.length > 0)
        throw Error(
            ".omit() cannot be used on object schemas containing refinements",
        );
    let i = new Set(Ie(e, t)),
        s = {};
    return (
        te(
            s,
            e,
            Reflect.ownKeys(W(e)).filter((c) => !i.has(c)),
        ),
        U(e, E(r, { shape: s, checks: [] }))
    );
}
function nc(e, t) {
    if (!ce(t)) throw Error("Invalid input to extend: expected a plain object");
    let r = e._zod.def.checks;
    if (r && r.length > 0) {
        let o = W(e);
        for (let i of Reflect.ownKeys(t))
            if (Object.getOwnPropertyDescriptor(o, i) !== void 0)
                throw Error(
                    "Cannot overwrite keys on object schemas containing refinements. Use `.safeExtend()` instead.",
                );
    }
    return U(e, E(e._zod.def, { shape: Er(e, t) }));
}
function Er(e, t) {
    let r = {};
    return (te(r, e, Reflect.ownKeys(W(e))), ec(r, t), r);
}
function oc(e, t) {
    if (!ce(t))
        throw Error("Invalid input to safeExtend: expected a plain object");
    return U(e, E(e._zod.def, { shape: Er(e, t) }));
}
function ic(e, t) {
    if (!t?._zod?.def)
        throw Error(
            "Invalid input to merge: expected an object schema. To merge a plain shape, use `.extend()`.",
        );
    if (e._zod.def.checks?.length)
        throw Error(
            ".merge() cannot be used on object schemas containing refinements. Use .safeExtend() instead.",
        );
    let r = {};
    (te(r, e, Reflect.ownKeys(W(e))), te(r, t, Reflect.ownKeys(W(t))));
    let n = E(e._zod.def, {
        shape: r,
        get catchall() {
            return t._zod.def.catchall;
        },
        checks: t._zod.def.checks ?? [],
    });
    return U(e, n);
}
function Tr(e, t, r, n = "partial") {
    let i = t._zod.def.checks;
    if (i && i.length > 0)
        throw Error(
            `.${n}() cannot be used on object schemas containing refinements`,
        );
    let c = r ? new Set(Ie(t, r)) : void 0,
        a = {};
    return (
        te(
            a,
            t,
            Reflect.ownKeys(W(t)),
            e &&
                ((u, l) =>
                    c && !c.has(l)
                        ? u
                        : new e({ type: "optional", innerType: u })),
        ),
        U(t, E(t._zod.def, { shape: a, checks: [] }))
    );
}
function sc(e, t, r) {
    let n = r ? new Set(Ie(t, r)) : void 0,
        o = {};
    return (
        te(o, t, Reflect.ownKeys(W(t)), (i, s) =>
            n && !n.has(s) ? i : new e({ type: "nonoptional", innerType: i }),
        ),
        U(t, E(t._zod.def, { shape: o }))
    );
}
function V(e, t = 0) {
    if (e.aborted === !0) return !0;
    for (let r = t; r < e.issues.length; r++)
        if (e.issues[r]?.continue !== !0) return !0;
    return !1;
}
function Ir(e, t = 0) {
    if (e.aborted === !0) return !0;
    for (let r = t; r < e.issues.length; r++)
        if (e.issues[r]?.continue === !1) return !0;
    return !1;
}
function Ne(e, t) {
    return t.map((r) => {
        var n;
        return ((n = r).path ?? (n.path = []), r.path.unshift(e), r);
    });
}
function he(e) {
    return typeof e === "string" ? e : e?.message;
}
function $t(e) {
    let t = [];
    for (let r of e ?? []) {
        let n = r._zod.def;
        if (!/_(length|size)$|_equals$/.test(n.check)) break;
        let o = n.maximum ?? n.length ?? n.size;
        if (o >= 0) t.push([o, r]);
    }
    return t;
}
function De(e, t, r) {
    var n;
    for (let o = t; o < e.length; o++) (n = e[o]).schema ?? (n.schema = r);
}
function Y(e, t, r) {
    var n;
    let o = e.inst?._zod?.traits;
    if (o?.has("$ZodType"))
        if (o.has("$ZodCheck")) (n = e).schema ?? (n.schema = e.inst);
        else e.schema = e.inst;
    let i = e.schema !== e.inst ? e.schema?._zod.def?.error : void 0,
        s = e.message
            ? e.message
            : (he(e.inst?._zod.def?.error?.(e)) ??
              he(i?.(e)) ??
              he(t?.error?.(e)) ??
              he(r.customError?.(e)) ??
              he(r.localeError?.(e)) ??
              "Invalid input"),
        c = {};
    for (let a of Object.keys(e)) {
        if (
            a === "inst" ||
            a === "schema" ||
            a === "continue" ||
            a === "input" ||
            a === "__proto__"
        )
            continue;
        c[a] = e[a];
    }
    if ((c.path ?? (c.path = []), (c.message = s), t?.reportInput))
        c.input = e.input;
    return c;
}
var cc = /[\uD800-\uDBFF]/;
function Re(e) {
    let t = e.length;
    if (!cc.test(e)) return t;
    let r = t;
    for (let n = 0; n < t - 1; n++)
        if (
            (e.charCodeAt(n) & 64512) === 55296 &&
            (e.charCodeAt(n + 1) & 64512) === 56320
        )
            (r--, n++);
    return r;
}
function je(e) {
    if (Array.isArray(e)) return "array";
    if (typeof e === "string") return "string";
    return "unknown";
}
function Nr(e) {
    let t = typeof e;
    switch (t) {
        case "number":
            return Number.isNaN(e) ? "nan" : "number";
        case "object": {
            if (e === null) return "null";
            if (Array.isArray(e)) return "array";
            let r = e;
            if (
                r &&
                Object.getPrototypeOf(r) !== Object.prototype &&
                "constructor" in r &&
                r.constructor
            )
                return r.constructor.name;
        }
    }
    return t;
}
function re(...e) {
    let [t, r, n] = e;
    if (typeof t === "string")
        return { message: t, code: "custom", input: r, inst: n };
    return { ...t };
}
function Dr(e, t) {
    for (let r in t) {
        let n = Object.getOwnPropertyDescriptor(t, r);
        if (n.get) Object.defineProperty(e, r, { ...n, enumerable: !1 });
        else uc(e, r, n.value);
    }
}
function J(e, t, r, n = !0) {
    return (
        Object.defineProperty(e, t, {
            configurable: !0,
            writable: !0,
            enumerable: n,
            value: r,
        }),
        r
    );
}
function Fe(e, t, r) {
    return J(e, t, r, !1);
}
function Rr(e, t) {
    for (let r in e) {
        let n = e[r];
        Object.defineProperty(t, r, {
            configurable: !0,
            enumerable: !0,
            get() {
                return J(this, r, n(this));
            },
            set(o) {
                J(this, r, o);
            },
        });
    }
    return t;
}
function uc(e, t, r) {
    Object.defineProperty(e, t, {
        configurable: !0,
        get() {
            return this == null ? r : J(this, t, r.bind(this));
        },
        set(n) {
            J(this, t, n);
        },
    });
}
function ac(e, t) {
    let r = Object.getPrototypeOf(e);
    return t in r ? void 0 : r;
}
var gt,
    q = !1,
    lc = {
        configurable: !0,
        get() {
            q = !0;
            return;
        },
    };
function x(e, t, r) {
    let n = Object.getPrototypeOf(e._zod);
    if (t in n && gt !== e._zod) {
        gt = void 0;
        return;
    }
    ((gt = e._zod),
        Object.defineProperty(n, t, {
            configurable: !0,
            get() {
                Object.defineProperty(this, t, lc);
                let o = q;
                q = !1;
                try {
                    let i = r(this);
                    if (q) delete this[t];
                    else
                        Object.defineProperty(this, t, {
                            configurable: !0,
                            writable: !0,
                            value: i,
                        });
                    return ((q = q || o), i);
                } catch (i) {
                    throw (delete this[t], (q = q || o), i);
                }
            },
            set(o) {
                Object.defineProperty(this, t, {
                    configurable: !0,
                    writable: !0,
                    value: o,
                });
            },
        }));
}
function pc(e, t, r, n) {
    let o = ac(e, t);
    if (!o) return;
    Object.defineProperty(o, t, {
        configurable: !0,
        get() {
            let i = {
                configurable: !0,
                writable: !0,
                enumerable: n,
                value: void 0,
            };
            return (
                Object.defineProperty(this, t, i),
                (i.value = r(this)),
                Object.defineProperty(this, t, i),
                i.value
            );
        },
        set(i) {
            Object.defineProperty(this, t, {
                configurable: !0,
                writable: !0,
                enumerable: n,
                value: i,
            });
        },
    });
}
var fc = "~constantCatch";
function jr(e) {
    let t = () => e;
    return ((t[fc] = !0), t);
}
var Fr;
var zt = { value: void 0, enumerable: !1 },
    Lr = "captureStackTrace" in Error ? Error : null;
function dc(e) {
    let t = Lr;
    if (t) {
        let r = t.stackTraceLimit;
        if (typeof r === "number") {
            try {
                t.stackTraceLimit = 0;
            } catch {
                return ((Lr = null), new e());
            }
            try {
                return new e();
            } finally {
                t.stackTraceLimit = r;
            }
        }
    }
    return new e();
}
function p(e, t, r, n) {
    let o = {};
    function i(d) {
        ((this.def = d), (this.constr = f), (this.traits = new Set()));
    }
    i.prototype = o;
    let s = r,
        c = s && new WeakSet();
    function a(d, h) {
        if (!d._zod) {
            zt.value = new i(h);
            try {
                Object.defineProperty(d, "_zod", zt);
            } finally {
                zt.value = void 0;
            }
        } else if (d._zod.traits.has(e)) return;
        if ((d._zod.traits.add(e), t(d, h), c)) {
            let y = Object.getPrototypeOf(d),
                _ = d._zod.constr.prototype,
                v = y;
            while (v && v !== _) v = Object.getPrototypeOf(v);
            let L = v ?? y;
            if (!c.has(L)) (c.add(L), Dr(L, s));
        }
        let m = f.prototype;
        for (let y in m) {
            if (!Object.prototype.hasOwnProperty.call(m, y)) continue;
            if (!(y in d)) d[y] = m[y].bind(d);
        }
    }
    let u = n?.Parent ?? Object;
    class l extends u {}
    Object.defineProperty(l, "name", { value: e });
    function f(d) {
        let h = n?.Parent ? dc(l) : this;
        a(h, d);
        let m = h._zod.deferred;
        if (m) {
            for (let _ of m) _();
            h._zod.deferred = void 0;
        }
        let y = globalThis.__zod_globalConfig?.postProcessor;
        if (y) y(h);
        return h;
    }
    return (
        Object.defineProperty(f, "init", { value: a }),
        Object.defineProperty(f, Symbol.hasInstance, {
            value: (d) => {
                if (n?.Parent && d instanceof n.Parent) return !0;
                return d?._zod?.traits?.has(e);
            },
        }),
        Object.defineProperty(f, "name", { value: e }),
        f
    );
}
class H extends Error {
    constructor() {
        super(
            "Encountered Promise during synchronous parse. Use .parseAsync() instead.",
        );
    }
}
class xe extends Error {
    constructor(e) {
        super(`Encountered unidirectional transform during encode: ${e}`);
        this.name = "ZodEncodeError";
    }
}
(Fr = globalThis).__zod_globalConfig ?? (Fr.__zod_globalConfig = {});
var T = globalThis.__zod_globalConfig;
function j(e) {
    if (e) Object.assign(T, e);
    return T;
}
function hc() {
    let e = this._zod;
    return (e.message ?? (e.message = JSON.stringify(e.def, ge, 2)), e.message);
}
function mc(e) {
    this._zod.message = e;
}
var gc = { get: hc, set: mc, enumerable: !0, configurable: !0 },
    kt = { value: void 0, enumerable: !1 },
    Mr = new WeakSet([Object.prototype, Error.prototype]),
    Ur = (e, t) => {
        ((e.name = "$ZodError"),
            (kt.value = t),
            Object.defineProperty(e, "issues", kt),
            (kt.value = void 0),
            Object.defineProperty(e, "message", gc));
        let r = Object.getPrototypeOf(e);
        if (!Mr.has(r))
            (Mr.add(r),
                Object.defineProperty(r, "toString", {
                    configurable: !0,
                    enumerable: !1,
                    get() {
                        let n = () => this.message;
                        return (
                            Object.defineProperty(this, "toString", {
                                value: n,
                                configurable: !0,
                                writable: !0,
                            }),
                            n
                        );
                    },
                    set(n) {
                        Object.defineProperty(this, "toString", {
                            value: n,
                            configurable: !0,
                            writable: !0,
                        });
                    },
                }));
    },
    Vr = p("$ZodError", Ur),
    Ja = p("$ZodError", Ur, void 0, { Parent: Error });
function _c(e, t, r) {
    if (!Object.prototype.hasOwnProperty.call(e, t))
        if (t === "__proto__")
            Object.defineProperty(e, t, {
                value: r(),
                writable: !0,
                enumerable: !0,
                configurable: !0,
            });
        else e[t] = r();
    return e[t];
}
function Hr(e, t = (r) => r.message) {
    let r = {},
        n = [];
    for (let o of e.issues)
        if (o.path.length > 0) _c(r, o.path[0], () => []).push(t(o));
        else n.push(t(o));
    return { formErrors: n, fieldErrors: r };
}
function Br(e, t = (r) => r.message) {
    let r = { _errors: [] },
        n = (o, i = []) => {
            for (let s of o.issues)
                if (s.code === "invalid_union" && s.errors.length)
                    s.errors.map((c) => n({ issues: c }, [...i, ...s.path]));
                else if (s.code === "invalid_key")
                    n({ issues: s.issues }, [...i, ...s.path]);
                else if (s.code === "invalid_element")
                    n({ issues: s.issues }, [...i, ...s.path]);
                else {
                    let c = [...i, ...s.path];
                    if (c.length === 0) r._errors.push(t(s));
                    else {
                        let a = r,
                            u = 0;
                        while (u < c.length) {
                            let l = c[u],
                                f = u === c.length - 1;
                            if (l === "_errors") {
                                if (f) a._errors.push(t(s));
                                u++;
                                continue;
                            }
                            if (!Object.prototype.hasOwnProperty.call(a, l))
                                Object.defineProperty(a, l, {
                                    value: { _errors: [] },
                                    enumerable: !0,
                                    writable: !0,
                                    configurable: !0,
                                });
                            let d = a[l];
                            if (f) d._errors.push(t(s));
                            ((a = d), u++);
                        }
                    }
                }
        };
    return (n(e), r);
}
function Le(e, t) {
    return { callee: t?.callee ?? e, Err: t?.Err };
}
var Me = (e) => {
    let t = (r, n, o, i) => {
        let s = o ? { ...o, async: !1 } : { async: !1 },
            c = r._zod.run({ value: n, issues: [] }, s);
        if (c instanceof Promise) throw new H();
        if (c.issues.length) {
            let a = new (i?.Err ?? e)(c.issues.map((u) => Y(u, s, j())));
            throw (yt(a, i?.callee ?? t), a);
        }
        return c.value;
    };
    return t;
};
var Ue = (e) => {
    let t = async (r, n, o, i) => {
        let s = o ? { ...o, async: !0 } : { async: !0 },
            c = r._zod.run({ value: n, issues: [] }, s);
        if (c instanceof Promise) c = await c;
        if (c.issues.length) {
            let a = new (i?.Err ?? e)(c.issues.map((u) => Y(u, s, j())));
            throw (yt(a, i?.callee ?? t), a);
        }
        return c.value;
    };
    return t;
};
var Ve = (e) => (t, r, n) => {
    let o = n ? { ...n, async: !1 } : { async: !1 },
        i = t._zod.run({ value: r, issues: [] }, o);
    if (i instanceof Promise) throw new H();
    return i.issues.length
        ? Wr(e, i.issues, o)
        : { success: !0, data: i.value };
};
function Wr(e, t, r) {
    let n;
    return {
        success: !1,
        get error() {
            if (!n)
                ((n = new e(t.map((o) => Y(o, r, j())))),
                    (t = void 0),
                    (r = void 0));
            return n;
        },
        set error(o) {
            ((n = o), (t = void 0), (r = void 0));
        },
    };
}
var He = (e) => async (t, r, n) => {
    let o = n ? { ...n, async: !0 } : { async: !0 },
        i = t._zod.run({ value: r, issues: [] }, o);
    if (i instanceof Promise) i = await i;
    return i.issues.length
        ? Wr(e, i.issues, o)
        : { success: !0, data: i.value };
};
var yc = Symbol.for("zod.compile.invalid"),
    xc = Symbol.for("zod.compile.fallback"),
    Ot = (e, t, r) => {
        let n = e._zod.bag.validator;
        if (n !== void 0) {
            if (n(t) !== yc) return !0;
            if (n.definite === !0 && r === void 0) return !1;
        }
        return bc(e, t, r);
    };
function bc(e, t, r) {
    let n = r
            ? { ...r, async: !1, abortEarly: !0 }
            : { async: !1, abortEarly: !0 },
        o = e._zod.bag.fallbackRun,
        i;
    if (o) ((n[xc] = !0), (i = o({ value: t, issues: [] }, n)));
    else i = e._zod.run({ value: t, issues: [] }, n);
    if (i instanceof Promise) throw new H();
    return i.issues.length === 0;
}
var St = async (e, t, r) => {
        let n = r
                ? { ...r, async: !0, abortEarly: !0 }
                : { async: !0, abortEarly: !0 },
            o = e._zod.run({ value: t, issues: [] }, n);
        if (o instanceof Promise) o = await o;
        return o.issues.length === 0;
    },
    Jr = (e) => {
        let t = Me(e),
            r = (n, o, i, s) => {
                let c = i
                    ? { ...i, direction: "backward" }
                    : { direction: "backward" };
                return t(n, o, c, Le(r, s));
            };
        return r;
    };
var Gr = (e) => {
    let t = Me(e),
        r = (n, o, i, s) => t(n, o, i, Le(r, s));
    return r;
};
var Kr = (e) => {
    let t = Ue(e),
        r = async (n, o, i, s) => {
            let c = i
                ? { ...i, direction: "backward" }
                : { direction: "backward" };
            return await t(n, o, c, Le(r, s));
        };
    return r;
};
var qr = (e) => {
    let t = Ue(e),
        r = async (n, o, i, s) => await t(n, o, i, Le(r, s));
    return r;
};
var Xr = (e) => (t, r, n) => {
    let o = n ? { ...n, direction: "backward" } : { direction: "backward" };
    return Ve(e)(t, r, o);
};
var Yr = (e) => (t, r, n) => Ve(e)(t, r, n);
var Qr = (e) => async (t, r, n) => {
    let o = n ? { ...n, direction: "backward" } : { direction: "backward" };
    return He(e)(t, r, o);
};
var en = (e) => async (t, r, n) => He(e)(t, r, n);
var tn = /^[cC][0-9a-z]{6,}$/,
    rn = /^[0-9a-z]+$/,
    nn = /^[0-7][0-9A-HJKMNP-TV-Za-hjkmnp-tv-z]{25}$/,
    on = /^[0-9a-vA-V]{20}$/,
    sn = /^[A-Za-z0-9]{27}$/,
    cn = /^[a-zA-Z0-9_-]{21}$/;
function un(e) {
    return new RegExp(`^[a-zA-Z0-9_-]{${e}}$`);
}
var an =
    /^P(?:(\d+W)|(?!.*W)(?=\d|T\d)(\d+Y)?(\d+M)?(\d+D)?(T(?=\d)(\d+H)?(\d+M)?(\d+([.,]\d+)?S)?)?)$/;
var ln =
        /^([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})$/,
    Zt = (e) => {
        if (!e)
            return /^([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}|00000000-0000-0000-0000-000000000000|ffffffff-ffff-ffff-ffff-ffffffffffff)$/;
        return new RegExp(
            `^([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-${e}[0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12})$`,
        );
    };
var pn =
    /^(?:[A-Za-z0-9_'+\-]+\.)*[A-Za-z0-9_'+\-]*[A-Za-z0-9_+-]@(?:[A-Za-z0-9][A-Za-z0-9\-]*\.)+[A-Za-z]{2,}$/;
var vc =
    "^(?=[\\s\\S]*[\\p{Extended_Pictographic}\\p{Regional_Indicator}\\u20E3])[\\p{Extended_Pictographic}\\p{Emoji_Component}]+$";
function fn() {
    return new RegExp(vc, "u");
}
var dn =
        /^(?:(?:25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])\.){3}(?:25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])$/,
    hn =
        /^(([0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}|([0-9a-fA-F]{1,4}:){1,7}:|([0-9a-fA-F]{1,4}:){1,6}:[0-9a-fA-F]{1,4}|([0-9a-fA-F]{1,4}:){1,5}(:[0-9a-fA-F]{1,4}){1,2}|([0-9a-fA-F]{1,4}:){1,4}(:[0-9a-fA-F]{1,4}){1,3}|([0-9a-fA-F]{1,4}:){1,3}(:[0-9a-fA-F]{1,4}){1,4}|([0-9a-fA-F]{1,4}:){1,2}(:[0-9a-fA-F]{1,4}){1,5}|[0-9a-fA-F]{1,4}:((:[0-9a-fA-F]{1,4}){1,6})|:((:[0-9a-fA-F]{1,4}){1,7}|:))$/;
var mn =
        /^((25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])\.){3}(25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])\/([0-9]|[1-2][0-9]|3[0-2])$/,
    gn =
        /^(([0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}|([0-9a-fA-F]{1,4}:){1,7}:|([0-9a-fA-F]{1,4}:){1,6}:[0-9a-fA-F]{1,4}|([0-9a-fA-F]{1,4}:){1,5}(:[0-9a-fA-F]{1,4}){1,2}|([0-9a-fA-F]{1,4}:){1,4}(:[0-9a-fA-F]{1,4}){1,3}|([0-9a-fA-F]{1,4}:){1,3}(:[0-9a-fA-F]{1,4}){1,4}|([0-9a-fA-F]{1,4}:){1,2}(:[0-9a-fA-F]{1,4}){1,5}|[0-9a-fA-F]{1,4}:((:[0-9a-fA-F]{1,4}){1,6})|:((:[0-9a-fA-F]{1,4}){1,7}|:))\/(12[0-8]|1[01][0-9]|[1-9]?[0-9])$/,
    _n =
        /^$|^(?:[0-9a-zA-Z+/]{4})*(?:(?:[0-9a-zA-Z+/]{2}==)|(?:[0-9a-zA-Z+/]{3}=))?$/,
    yn = /^(?:[A-Za-z0-9_-]{4})*(?:[A-Za-z0-9_-]{2,3})?$/;
var xn = /^https?$/,
    bn = /^\+[1-9]\d{6,14}$/;
var vn =
    "(?:(?:\\d\\d[2468][048]|\\d\\d[13579][26]|\\d\\d0[48]|[02468][048]00|[13579][26]00)-02-29|\\d{4}-(?:(?:0[13578]|1[02])-(?:0[1-9]|[12]\\d|3[01])|(?:0[469]|11)-(?:0[1-9]|[12]\\d|30)|(?:02)-(?:0[1-9]|1\\d|2[0-8])))";
function $c(e) {
    return new RegExp(`^${e}$`);
}
var $n = $c(vn);
function Pt(e) {
    return typeof e.precision === "number"
        ? e.precision === -1
            ? "(?:[01]\\d|2[0-3]):[0-5]\\d"
            : e.precision === 0
              ? "(?:[01]\\d|2[0-3]):[0-5]\\d:[0-5]\\d"
              : `(?:[01]\\d|2[0-3]):[0-5]\\d:[0-5]\\d\\.\\d{${e.precision}}`
        : e.seconds
          ? "(?:[01]\\d|2[0-3]):[0-5]\\d:[0-5]\\d(?:\\.\\d+)?"
          : "(?:[01]\\d|2[0-3]):[0-5]\\d(?::[0-5]\\d(?:\\.\\d+)?)?";
}
function zn(e) {
    return new RegExp(`^${Pt(e)}$`);
}
function wn(e) {
    let t = ["Z"];
    if (e.offset) t.push("([+-](?:[01]\\d|2[0-3]):[0-5]\\d)");
    let r = `${Pt({ precision: e.precision, seconds: !0 })}(?:${t.join("|")})`,
        n = e.local ? `${r}|${Pt({ precision: e.precision })}` : r;
    return new RegExp(`^${vn}T(?:${n})$`);
}
var kn = /^[\s\S]{0,}$/;
var At = /^-?\d+(?:\.\d+)?$/,
    On = /^(?:true|false)$/i;
var Sn = /^[^A-Z]*$/,
    Pn = /^[^a-z]*$/;
var A = p("$ZodCheck", (e, t) => {
    var r;
    (e._zod ?? (e._zod = {}),
        (e._zod.def = t),
        (r = e._zod).onattach ?? (r.onattach = []));
});
var Ct = (e) => {
        let t = e.value;
        return !zr(t) && t.length !== void 0;
    },
    Be = { number: "number", bigint: "bigint", object: "date" },
    Et = p("$ZodCheckLessThan", (e, t) => {
        A.init(e, t);
        let r = Be[typeof t.value];
        e._zod.check = (n) => {
            if (t.inclusive ? n.value <= t.value : n.value < t.value) return;
            n.issues.push({
                origin: Be[typeof n.value] ?? r,
                code: "too_big",
                maximum:
                    typeof t.value === "object" ? t.value.getTime() : t.value,
                input: n.value,
                inclusive: t.inclusive,
                inst: e,
                continue: !t.abort,
            });
        };
    }),
    Tt = p("$ZodCheckGreaterThan", (e, t) => {
        A.init(e, t);
        let r = Be[typeof t.value];
        e._zod.check = (n) => {
            if (t.inclusive ? n.value >= t.value : n.value > t.value) return;
            n.issues.push({
                origin: Be[typeof n.value] ?? r,
                code: "too_small",
                minimum:
                    typeof t.value === "object" ? t.value.getTime() : t.value,
                input: n.value,
                inclusive: t.inclusive,
                inst: e,
                continue: !t.abort,
            });
        };
    }),
    Zn = p("$ZodCheckMultipleOf", (e, t) => {
        (A.init(e, t),
            (e._zod.check = (r) => {
                if (typeof r.value !== typeof t.value)
                    throw Error(
                        "Cannot mix number and bigint in multiple_of check.",
                    );
                if (
                    typeof r.value === "bigint"
                        ? t.value !== BigInt(0) &&
                          r.value % t.value === BigInt(0)
                        : wr(r.value, t.value) === 0
                )
                    return;
                r.issues.push({
                    origin: typeof r.value,
                    code: "not_multiple_of",
                    divisor: t.value,
                    input: r.value,
                    inst: e,
                    continue: !t.abort,
                });
            }));
    }),
    An = p("$ZodCheckNumberFormat", (e, t) => {
        (A.init(e, t), (t.format = t.format || "float64"));
        let r = t.format?.includes("int"),
            n = r ? "int" : "number",
            [o, i] = Te[t.format];
        e._zod.check = (s) => {
            let c = s.value;
            if (r) {
                if (!Number.isInteger(c)) {
                    s.issues.push({
                        expected: n,
                        format: t.format,
                        code: "invalid_type",
                        continue: !1,
                        input: c,
                        inst: e,
                    });
                    return;
                }
                if (!Number.isSafeInteger(c)) {
                    if (c > 0)
                        s.issues.push({
                            input: c,
                            code: "too_big",
                            maximum: Number.MAX_SAFE_INTEGER,
                            note: "Integers must be within the safe integer range.",
                            inst: e,
                            origin: n,
                            inclusive: !0,
                            continue: !t.abort,
                        });
                    else
                        s.issues.push({
                            input: c,
                            code: "too_small",
                            minimum: Number.MIN_SAFE_INTEGER,
                            note: "Integers must be within the safe integer range.",
                            inst: e,
                            origin: n,
                            inclusive: !0,
                            continue: !t.abort,
                        });
                    return;
                }
            }
            if (c < o)
                s.issues.push({
                    origin: "number",
                    input: c,
                    code: "too_small",
                    minimum: o,
                    inclusive: !0,
                    inst: e,
                    continue: !t.abort,
                });
            if (c > i)
                s.issues.push({
                    origin: "number",
                    input: c,
                    code: "too_big",
                    maximum: i,
                    inclusive: !0,
                    inst: e,
                    continue: !t.abort,
                });
        };
    });
var Cn = p("$ZodCheckMaxLength", (e, t) => {
        var r;
        (A.init(e, t),
            (r = e._zod.def).when ?? (r.when = Ct),
            (e._zod.check = (n) => {
                let o = n.value,
                    i = o.length;
                if (
                    (typeof o === "string" && i > t.maximum ? Re(o) : i) <=
                    t.maximum
                )
                    return;
                let c = je(o);
                n.issues.push({
                    origin: c,
                    code: "too_big",
                    maximum: t.maximum,
                    inclusive: !0,
                    input: o,
                    inst: e,
                    continue: !t.abort,
                });
            }));
    }),
    En = p("$ZodCheckMinLength", (e, t) => {
        var r;
        (A.init(e, t),
            (r = e._zod.def).when ?? (r.when = Ct),
            (e._zod.check = (n) => {
                let o = n.value,
                    i = o.length;
                if (
                    (typeof o === "string" &&
                    i >= t.minimum &&
                    i < t.minimum * 2
                        ? Re(o)
                        : i) >= t.minimum
                )
                    return;
                let c = je(o);
                n.issues.push({
                    origin: c,
                    code: "too_small",
                    minimum: t.minimum,
                    inclusive: !0,
                    input: o,
                    inst: e,
                    continue: !t.abort,
                });
            }));
    }),
    Tn = p("$ZodCheckLengthEquals", (e, t) => {
        var r;
        (A.init(e, t),
            (r = e._zod.def).when ?? (r.when = Ct),
            (e._zod.check = (n) => {
                let o = n.value,
                    i = o.length,
                    s =
                        typeof o === "string" &&
                        i >= t.length &&
                        i <= t.length * 2
                            ? Re(o)
                            : i;
                if (s === t.length) return;
                let c = je(o),
                    a = s > t.length;
                n.issues.push({
                    origin: c,
                    ...(a
                        ? { code: "too_big", maximum: t.length }
                        : { code: "too_small", minimum: t.length }),
                    inclusive: !0,
                    exact: !0,
                    input: n.value,
                    inst: e,
                    continue: !t.abort,
                });
            }));
    }),
    ve = p("$ZodCheckStringFormat", (e, t) => {
        var r, n;
        if ((A.init(e, t), t.pattern))
            (r = e._zod).check ??
                (r.check = (o) => {
                    if (((t.pattern.lastIndex = 0), t.pattern.test(o.value)))
                        return;
                    o.issues.push({
                        origin: "string",
                        code: "invalid_format",
                        format: t.format,
                        input: o.value,
                        ...(t.pattern ? { pattern: t.pattern.toString() } : {}),
                        inst: e,
                        continue: !t.abort,
                    });
                });
        else (n = e._zod).check ?? (n.check = () => {});
    }),
    In = p("$ZodCheckRegex", (e, t) => {
        (ve.init(e, t),
            (e._zod.check = (r) => {
                if (((t.pattern.lastIndex = 0), t.pattern.test(r.value)))
                    return;
                r.issues.push({
                    origin: "string",
                    code: "invalid_format",
                    format: "regex",
                    input: r.value,
                    pattern: t.pattern.toString(),
                    inst: e,
                    continue: !t.abort,
                });
            }));
    }),
    Nn = p("$ZodCheckLowerCase", (e, t) => {
        (t.pattern ?? (t.pattern = Sn), ve.init(e, t));
    }),
    Dn = p("$ZodCheckUpperCase", (e, t) => {
        (t.pattern ?? (t.pattern = Pn), ve.init(e, t));
    }),
    Rn = p("$ZodCheckIncludes", (e, t) => {
        A.init(e, t);
        let r = X(t.includes),
            n = new RegExp(
                typeof t.position === "number" ? `^.{${t.position},}${r}` : r,
            );
        ((t.pattern = n),
            (e._zod.check = (o) => {
                if (o.value.includes(t.includes, t.position)) return;
                o.issues.push({
                    origin: "string",
                    code: "invalid_format",
                    format: "includes",
                    includes: t.includes,
                    input: o.value,
                    inst: e,
                    continue: !t.abort,
                });
            }));
    }),
    jn = p("$ZodCheckStartsWith", (e, t) => {
        A.init(e, t);
        let r = new RegExp(`^${X(t.prefix)}.*`);
        (t.pattern ?? (t.pattern = r),
            (e._zod.check = (n) => {
                if (n.value.startsWith(t.prefix)) return;
                n.issues.push({
                    origin: "string",
                    code: "invalid_format",
                    format: "starts_with",
                    prefix: t.prefix,
                    input: n.value,
                    inst: e,
                    continue: !t.abort,
                });
            }));
    }),
    Fn = p("$ZodCheckEndsWith", (e, t) => {
        A.init(e, t);
        let r = new RegExp(`.*${X(t.suffix)}$`);
        (t.pattern ?? (t.pattern = r),
            (e._zod.check = (n) => {
                if (n.value.endsWith(t.suffix)) return;
                n.issues.push({
                    origin: "string",
                    code: "invalid_format",
                    format: "ends_with",
                    suffix: t.suffix,
                    input: n.value,
                    inst: e,
                    continue: !t.abort,
                });
            }));
    });
var Ln = p("$ZodCheckOverwrite", (e, t) => {
    (A.init(e, t),
        (e._zod.check = (r) => {
            r.value = t.tx(r.value);
        }));
});
class It {
    constructor(e = [], t = {}) {
        ((this.content = []),
            (this.indent = 0),
            (this.args = e),
            (this.closed = t));
    }
    indented(e) {
        this.indent += 1;
        try {
            e(this);
        } finally {
            this.indent -= 1;
        }
    }
    write(e) {
        if (typeof e === "function") {
            (e(this, { execution: "sync" }), e(this, { execution: "async" }));
            return;
        }
        let r = e
                .split(`
`)
                .filter((i) => i),
            n = Math.min(...r.map((i) => i.length - i.trimStart().length)),
            o = r
                .map((i) => i.slice(n))
                .map((i) => " ".repeat(this.indent * 2) + i);
        for (let i of o) this.content.push(i);
    }
    compile() {
        let e = Function,
            t = this?.content ?? [""];
        return new e(
            ...Object.keys(this.closed),
            `return function (${this.args.join(", ")}) {
${t.join(`
`)}
};`,
        )(...Object.values(this.closed));
    }
}
var Un = { major: 4, minor: 6, patch: 7 };
var k = p(
        "$ZodType",
        (e, t) => {
            var r;
            (e ?? (e = {}),
                (e._zod.def = t),
                (e._zod.bag = e._zod.bag || {}),
                (e._zod.version = Un));
            let n = e._zod.def.checks,
                o = e._zod.traits.has("$ZodCheck")
                    ? [e, ...(n ?? [])]
                    : n?.length
                      ? [...n]
                      : [];
            for (let i of o) for (let s of i._zod.onattach) s(e);
            if (o.length === 0)
                ((r = e._zod).deferred ?? (r.deferred = []),
                    e._zod.deferred?.push(() => {
                        e._zod.run = e._zod.parse;
                    }));
            else {
                let i = (c, a, u) => {
                        if (c.memo) return c;
                        let l = V(c),
                            f;
                        for (let d of a) {
                            if (d._zod.def.when) {
                                if (Ir(c)) continue;
                                if (!d._zod.def.when(c)) continue;
                            } else if (l) continue;
                            let h = c.issues.length,
                                m = d._zod.check(c);
                            if (m instanceof Promise && u?.async === !1)
                                throw new H();
                            if (f || m instanceof Promise)
                                f = (f ?? Promise.resolve()).then(async () => {
                                    if ((await m, c.issues.length === h))
                                        return;
                                    if ((De(c.issues, h, e), !l)) l = V(c, h);
                                });
                            else {
                                if (c.issues.length === h) continue;
                                if ((De(c.issues, h, e), !l)) l = V(c, h);
                            }
                        }
                        if (f) return f.then(() => c);
                        return c;
                    },
                    s = (c, a, u) => {
                        if (V(c)) return ((c.aborted = !0), c);
                        let l = i(a, o, u);
                        if (l instanceof Promise) {
                            if (u.async === !1) throw new H();
                            return l.then((f) => e._zod.parse(f, u));
                        }
                        return e._zod.parse(l, u);
                    };
                e._zod.run = (c, a) => {
                    if (a.skipChecks) return e._zod.parse(c, a);
                    if (a.direction === "backward") {
                        let l = e._zod.parse(
                            { value: c.value, issues: [] },
                            { ...a, skipChecks: !0 },
                        );
                        if (l instanceof Promise)
                            return l.then((f) => s(f, c, a));
                        return s(l, c, a);
                    }
                    let u = e._zod.parse(c, a);
                    if (u instanceof Promise) {
                        if (a.async === !1) throw new H();
                        return u.then((l) => i(l, o, a));
                    }
                    return i(u, o, a);
                };
            }
        },
        {
            get "~standard"() {
                return Fe(this, "~standard", Dt(this));
            },
            set "~standard"(e) {
                J(this, "~standard", e);
            },
        },
    ),
    Qn = (e, t) =>
        e.issues.length
            ? { issues: e.issues.map((r) => Y(r, t, j())) }
            : { value: e.value };
async function wc(e, t) {
    let r = { async: !0 };
    return Qn(await e._zod.run({ value: t, issues: [] }, r), r);
}
function Dt(e) {
    return {
        validate: (t) => {
            let r = { async: !1 };
            try {
                let n = e._zod.run({ value: t, issues: [] }, r);
                if (!(n instanceof Promise)) return Qn(n, r);
            } catch (n) {}
            return wc(e, t);
        },
        vendor: "zod",
        version: 1,
    };
}
var Ke = p("$ZodString", (e, t) => {
        (k.init(e, t),
            (e._zod.pattern = t.pattern ?? kn),
            (e._zod.parse = (r, n) => {
                if (t.coerce)
                    try {
                        r.value = String(r.value);
                    } catch (o) {}
                if (typeof r.value === "string") return r;
                return (
                    r.issues.push({
                        expected: "string",
                        code: "invalid_type",
                        input: r.value,
                        inst: e,
                    }),
                    r
                );
            }));
    }),
    w = p("$ZodStringFormat", (e, t) => {
        (ve.init(e, t), Ke.init(e, t));
    }),
    eo = p("$ZodGUID", (e, t) => {
        (t.pattern ?? (t.pattern = ln), w.init(e, t));
    }),
    to = p("$ZodUUID", (e, t) => {
        if (t.version) {
            let n = { v1: 1, v2: 2, v3: 3, v4: 4, v5: 5, v6: 6, v7: 7, v8: 8 }[
                t.version
            ];
            if (n === void 0)
                throw Error(`Invalid UUID version: "${t.version}"`);
            t.pattern ?? (t.pattern = Zt(n));
        } else t.pattern ?? (t.pattern = Zt());
        w.init(e, t);
    }),
    ro = p("$ZodEmail", (e, t) => {
        (t.pattern ?? (t.pattern = pn), w.init(e, t));
    }),
    no = 1,
    Je = 2;
function oo(e) {
    try {
        if (typeof URL < "u" && typeof URL.canParse === "function")
            return URL.canParse(e);
        return (new URL(e), !0);
    } catch {
        return !1;
    }
}
function kc(e, t) {
    if (!("normalize" in t) && !("hostname" in t) && !("protocol" in t))
        return oo(e) || Je;
    return Oc(e, t);
}
function Oc(e, t) {
    if (
        !t.normalize &&
        t.protocol?.source === xn.source &&
        !/^https?:\/\//i.test(e)
    )
        return no;
    try {
        if (typeof URL < "u") {
            let r = URL;
            if (typeof r.parse === "function") return r.parse(e) ?? Je;
        }
        return new URL(e);
    } catch {
        return Je;
    }
}
var Sc = /[\t\n\r]/g;
function Vn(e) {
    return e.replace(Sc, "");
}
function Pc(e, t) {
    return ((t.lastIndex = 0), t.test(e.hostname));
}
function Zc(e, t) {
    return (
        (t.lastIndex = 0),
        t.test(e.protocol.endsWith(":") ? e.protocol.slice(0, -1) : e.protocol)
    );
}
var io = p("$ZodURL", (e, t) => {
        (w.init(e, t),
            (e._zod.check = (r) => {
                try {
                    let n = r.value.trim(),
                        o = kc(n, t);
                    if (o === no) {
                        r.issues.push({
                            code: "invalid_format",
                            format: "url",
                            note: "Invalid URL format",
                            input: r.value,
                            inst: e,
                            continue: !t.abort,
                        });
                        return;
                    }
                    if (o === Je) {
                        r.issues.push({
                            code: "invalid_format",
                            format: "url",
                            input: r.value,
                            inst: e,
                            continue: !t.abort,
                        });
                        return;
                    }
                    if (o === !0) {
                        r.value = Vn(n);
                        return;
                    }
                    if (t.hostname && !Pc(o, t.hostname))
                        r.issues.push({
                            code: "invalid_format",
                            format: "url",
                            note: "Invalid hostname",
                            pattern: t.hostname.source,
                            input: r.value,
                            inst: e,
                            continue: !t.abort,
                        });
                    if (t.protocol && !Zc(o, t.protocol))
                        r.issues.push({
                            code: "invalid_format",
                            format: "url",
                            note: "Invalid protocol",
                            pattern: t.protocol.source,
                            input: r.value,
                            inst: e,
                            continue: !t.abort,
                        });
                    r.value = t.normalize ? o.href : Vn(n);
                    return;
                } catch (n) {
                    r.issues.push({
                        code: "invalid_format",
                        format: "url",
                        input: r.value,
                        inst: e,
                        continue: !t.abort,
                    });
                }
            }));
    }),
    so = p("$ZodEmoji", (e, t) => {
        (t.pattern ?? (t.pattern = fn()), w.init(e, t));
    }),
    co = p("$ZodNanoID", (e, t) => {
        if (
            t.length !== void 0 &&
            (!Number.isInteger(t.length) || t.length < 1)
        )
            throw Error(`Invalid nanoid length: ${t.length}`);
        (t.pattern ?? (t.pattern = t.length === void 0 ? cn : un(t.length)),
            w.init(e, t));
    }),
    uo = p("$ZodCUID", (e, t) => {
        (t.pattern ?? (t.pattern = tn), w.init(e, t));
    }),
    ao = p("$ZodCUID2", (e, t) => {
        (t.pattern ?? (t.pattern = rn), w.init(e, t));
    }),
    lo = p("$ZodULID", (e, t) => {
        (t.pattern ?? (t.pattern = nn), w.init(e, t));
    }),
    po = p("$ZodXID", (e, t) => {
        (t.pattern ?? (t.pattern = on), w.init(e, t));
    }),
    fo = p("$ZodKSUID", (e, t) => {
        (t.pattern ?? (t.pattern = sn), w.init(e, t));
    }),
    ho = p("$ZodISODateTime", (e, t) => {
        (t.pattern ?? (t.pattern = wn(t)), w.init(e, t));
    }),
    mo = p("$ZodISODate", (e, t) => {
        (t.pattern ?? (t.pattern = $n), w.init(e, t));
    }),
    go = p("$ZodISOTime", (e, t) => {
        (t.pattern ?? (t.pattern = zn(t)), w.init(e, t));
    }),
    _o = p("$ZodISODuration", (e, t) => {
        (t.pattern ?? (t.pattern = an), w.init(e, t));
    }),
    yo = p("$ZodIPv4", (e, t) => {
        (t.pattern ?? (t.pattern = dn), w.init(e, t));
    }),
    Ac = /^[0-9a-fA-F:.]+$/;
function xo(e) {
    if (!Ac.test(e)) return !1;
    return oo(`http://[${e}]`);
}
var bo = p("$ZodIPv6", (e, t) => {
    (t.pattern ?? (t.pattern = hn),
        w.init(e, t),
        (e._zod.check = (r) => {
            if (!xo(r.value))
                r.issues.push({
                    code: "invalid_format",
                    format: "ipv6",
                    input: r.value,
                    inst: e,
                    continue: !t.abort,
                });
        }));
});
var vo = p("$ZodCIDRv4", (e, t) => {
    (t.pattern ?? (t.pattern = mn), w.init(e, t));
});
function Cc(e) {
    let t = e.split("/");
    if (t.length !== 2) return !1;
    let [r, n] = t;
    if (!n) return !1;
    let o = Number(n);
    if (`${o}` !== n) return !1;
    if (o < 0 || o > 128) return !1;
    return xo(r);
}
var $o = p("$ZodCIDRv6", (e, t) => {
    (t.pattern ?? (t.pattern = gn),
        w.init(e, t),
        (e._zod.check = (r) => {
            if (!Cc(r.value))
                r.issues.push({
                    code: "invalid_format",
                    format: "cidrv6",
                    input: r.value,
                    inst: e,
                    continue: !t.abort,
                });
        }));
});
function zo(e) {
    if (e === "") return !0;
    if (/\s/.test(e)) return !1;
    if (e.length % 4 !== 0) return !1;
    try {
        return (atob(e), !0);
    } catch {
        return !1;
    }
}
var Rt = /^[0-9a-zA-Z+/]*={0,2}$/,
    wo = p("$ZodBase64", (e, t) => {
        (t.pattern ?? (t.pattern = Rt),
            w.init(e, t),
            (e._zod.check = (r) => {
                if (zo(r.value)) return;
                r.issues.push({
                    code: "invalid_format",
                    format: "base64",
                    input: r.value,
                    inst: e,
                    continue: !t.abort,
                });
            }));
    }),
    qe = /^[A-Za-z0-9_-]*$/;
function Ec(e) {
    if (!qe.test(e)) return !1;
    let t = e.replace(/[-_]/g, (n) => (n === "-" ? "+" : "/")),
        r = t.padEnd(Math.ceil(t.length / 4) * 4, "=");
    return zo(r);
}
var ko = p("$ZodBase64URL", (e, t) => {
        (t.pattern ?? (t.pattern = qe),
            w.init(e, t),
            (e._zod.check = (r) => {
                if (Ec(r.value)) return;
                r.issues.push({
                    code: "invalid_format",
                    format: "base64url",
                    input: r.value,
                    inst: e,
                    continue: !t.abort,
                });
            }));
    }),
    Oo = p("$ZodE164", (e, t) => {
        (t.pattern ?? (t.pattern = bn), w.init(e, t));
    });
function Tc(e, t = null) {
    try {
        let r = e.split(".");
        if (r.length !== 3) return !1;
        let [n] = r;
        if (!n) return !1;
        let o = JSON.parse(atob(n));
        if ("typ" in o && o?.typ !== "JWT") return !1;
        if (!o.alg) return !1;
        if (t && (!("alg" in o) || o.alg !== t)) return !1;
        return !0;
    } catch {
        return !1;
    }
}
var So = p("$ZodJWT", (e, t) => {
    (w.init(e, t),
        (e._zod.check = (r) => {
            if (Tc(r.value, t.alg)) return;
            r.issues.push({
                code: "invalid_format",
                format: "jwt",
                input: r.value,
                inst: e,
                continue: !t.abort,
            });
        }));
});
var jt = p("$ZodNumber", (e, t) => {
        (k.init(e, t),
            (e._zod.pattern = At),
            (e._zod.parse = (r, n) => {
                if (t.coerce)
                    try {
                        r.value = Number(r.value);
                    } catch (s) {}
                let o = r.value;
                if (
                    typeof o === "number" &&
                    !Number.isNaN(o) &&
                    Number.isFinite(o)
                )
                    return r;
                let i =
                    typeof o === "number"
                        ? Number.isNaN(o)
                            ? "NaN"
                            : !Number.isFinite(o)
                              ? String(o)
                              : void 0
                        : void 0;
                return (
                    r.issues.push({
                        expected: "number",
                        code: "invalid_type",
                        input: o,
                        inst: e,
                        ...(i ? { received: i } : {}),
                    }),
                    r
                );
            }));
    }),
    Po = p("$ZodNumberFormat", (e, t) => {
        (An.init(e, t), jt.init(e, t));
    }),
    Zo = p("$ZodBoolean", (e, t) => {
        (k.init(e, t),
            (e._zod.pattern = On),
            (e._zod.parse = (r, n) => {
                if (t.coerce)
                    try {
                        r.value = Boolean(r.value);
                    } catch (i) {}
                let o = r.value;
                if (typeof o === "boolean") return r;
                return (
                    r.issues.push({
                        expected: "boolean",
                        code: "invalid_type",
                        input: o,
                        inst: e,
                    }),
                    r
                );
            }));
    });
var Ao = p("$ZodUnknown", (e, t) => {
        (k.init(e, t), (e._zod.parse = (r) => r));
    }),
    Co = p("$ZodNever", (e, t) => {
        (k.init(e, t),
            (e._zod.parse = (r, n) => (
                r.issues.push({
                    expected: "never",
                    code: "invalid_type",
                    input: r.value,
                    inst: e,
                }),
                r
            )));
    });
function Ic(e, t) {
    let r = $t(e._zod.def.checks);
    if (!r.length) return;
    let n = Math.min(...r.map((i) => i[0])),
        o = e._zod.parse;
    e._zod.parse = (i, s) => {
        let c = t(i.value);
        if (c > n && !s?.skipChecks && s?.direction !== "backward") {
            for (let [a, u] of $t(e._zod.def.checks))
                if (c > a && u._zod.def.when(i)) {
                    let l = i.issues.length;
                    return (
                        u._zod.check(i),
                        (i.issues[l].continue = !1),
                        De(i.issues, l, e),
                        i
                    );
                }
        }
        return o(i, s);
    };
}
var Nc = (e) => (Array.isArray(e) ? e.length : -1);
function Hn(e, t, r) {
    if (e.issues.length) t.issues.push(...Ne(r, e.issues));
    t.value[r] = e.value;
}
var Eo = p("$ZodArray", (e, t) => {
    k.init(e, t);
    let r = T.memoizer;
    (r?.attach(e),
        (e._zod.parse = (n, o) => {
            let i = n.value;
            if (!Array.isArray(i))
                return (
                    n.issues.push({
                        expected: "array",
                        code: "invalid_type",
                        input: i,
                        inst: e,
                    }),
                    n
                );
            n.value = r ? r.alloc(e, n, Array(i.length), o) : Array(i.length);
            let s = [],
                c = o?.abortEarly;
            for (let a = 0; a < i.length; a++) {
                let u = i[a],
                    l = t.element._zod.run({ value: u, issues: [] }, o);
                if (l instanceof Promise) s.push(l.then((f) => Hn(f, n, a)));
                else if ((Hn(l, n, a), c && l.issues.length !== 0 && V(l)))
                    break;
            }
            if (s.length) return Promise.all(s).then(() => n);
            return n;
        }),
        Ic(e, Nc));
});
function Ge(e, t, r, n, o, i) {
    let s = r in n,
        c = i === "optional";
    if (!s && c && o === "optional") return;
    if (e.issues.length) {
        if (o !== void 0 && c && !s) return;
        t.issues.push(...Ne(r, e.issues));
    }
    if (!s && o === void 0) {
        if (!e.issues.length)
            t.issues.push({
                code: "invalid_type",
                expected: "nonoptional",
                input: void 0,
                path: [r],
            });
        return;
    }
    if (e.value === void 0) {
        if (s || (o === "defaulted" && !c)) t.value[r] = void 0;
    } else t.value[r] = e.value;
}
var Dc = [];
function To(e) {
    let t = Object.keys(e.shape),
        r = Object.getOwnPropertySymbols(e.shape),
        n = r.length ? r : Dc,
        o = n.length ? [...t, ...n] : t;
    for (let s of o)
        if (!e.shape?.[s]?._zod?.traits?.has("$ZodType"))
            throw Error(
                `Invalid element at key "${String(s)}": expected a Zod schema`,
            );
    let i = Cr(e.shape);
    return {
        ...e,
        allKeys: o,
        symbolKeys: n,
        keySet: new Set(t),
        numKeys: t.length,
        optionalKeys: new Set(i),
    };
}
function Io(e, t, r, n, o, i, s) {
    let c = [],
        a = o.keySet,
        u = o.catchall._zod,
        l = u.def.type,
        { optin: f, optout: d } = u,
        h = 0;
    for (let m in t) {
        if (s && r.issues.length !== h) {
            if (V(r, h)) break;
            h = r.issues.length;
        }
        if (a.has(m)) continue;
        if (m === "__proto__") {
            if (l === "never") c.push(m);
            continue;
        }
        if (l === "never") {
            c.push(m);
            continue;
        }
        let y = u.run({ value: t[m], issues: [] }, n);
        if (y instanceof Promise) e.push(y.then((_) => Ge(_, r, m, t, f, d)));
        else Ge(y, r, m, t, f, d);
    }
    if (c.length)
        r.issues.push({
            code: "unrecognized_keys",
            keys: c,
            input: t,
            inst: i,
            continue: !0,
        });
    if (!e.length) return r;
    return Promise.all(e).then(() => r);
}
var Rc = p("$ZodObject", (e, t) => {
        k.init(e, t);
        let r = Object.getOwnPropertyDescriptor(t, "shape"),
            n = r?.get ? r.get.raw : (t.shape ?? {});
        if (n) {
            let u = () => {
                let l = { ...n };
                return (
                    Object.defineProperty(t, "shape", { value: l }),
                    (u.raw = l),
                    l
                );
            };
            ((u.raw = n), Object.defineProperty(t, "shape", { get: u }));
        }
        let o = _e(() => To(t));
        x(e, "propValues", (u) => {
            let l = u.def.shape,
                f = {};
            for (let d in l) {
                let h = l[d]._zod;
                if (h.values) {
                    if (!Object.prototype.hasOwnProperty.call(f, d))
                        D(f, d, new Set());
                    for (let m of h.values) f[d].add(m);
                    if (h.optin !== void 0) f[d].add(void 0);
                }
            }
            return f;
        });
        let i = se,
            s = t.catchall,
            c,
            a = T.memoizer;
        (a?.attach(e),
            (e._zod.parse = (u, l) => {
                c ?? (c = o.value);
                let f = u.value;
                if (!i(f))
                    return (
                        u.issues.push({
                            expected: "object",
                            code: "invalid_type",
                            input: f,
                            inst: e,
                        }),
                        u
                    );
                u.value = a ? a.alloc(e, u, {}, l) : {};
                let d = [],
                    h = c.shape,
                    m = l?.abortEarly,
                    y = u.issues.length;
                for (let _ of c.allKeys) {
                    if (m && u.issues.length !== y) {
                        if (V(u, y)) break;
                        y = u.issues.length;
                    }
                    if (_ === "__proto__") continue;
                    let v = h[_],
                        L = v._zod.optin,
                        pe = v._zod.optout,
                        fe = v._zod.run({ value: f[_], issues: [] }, l);
                    if (fe instanceof Promise)
                        d.push(fe.then((P) => Ge(P, u, _, f, L, pe)));
                    else Ge(fe, u, _, f, L, pe);
                }
                if (!s) return d.length ? Promise.all(d).then(() => u) : u;
                return Io(d, f, u, l, o.value, e, m === !0);
            }));
    }),
    No = p("$ZodObjectJIT", (e, t) => {
        Rc.init(e, t);
        let r = e._zod.parse,
            n = _e(() => To(t)),
            o = T.memoizer,
            i = (h) => {
                let m = n.value,
                    y = m.symbolKeys,
                    _ = new It(["payload", "ctx"], {
                        shape: h,
                        inst: e,
                        memo: o,
                        syms: y,
                    }),
                    v = (P) =>
                        `shape[${P}]._zod.run({ value: input[${P}], issues: [] }, ctx)`,
                    L = (P, z) => `
          let ${P}_ab = false;
          for (let i = 0; i < ${P}.issues.length; i++) {
            const iss = ${P}.issues[i];
            iss.path = iss.path ? [${z}, ...iss.path] : [${z}];
            payload.issues.push(iss);
            if (iss.continue !== true) ${P}_ab = true;
          }
          if (${P}_ab && ctx && ctx.abortEarly) {
            payload.value = newResult;
            return payload;
          }`;
                _.write("const input = payload.value;");
                let pe = Object.create(null),
                    fe = 0;
                for (let P of m.allKeys) pe[P] = `key_${fe++}`;
                _.write(
                    o
                        ? "const newResult = memo.alloc(inst, payload, {}, ctx);"
                        : "const newResult = {};",
                );
                for (let P of m.allKeys) {
                    if (P === "__proto__") continue;
                    let z = pe[P],
                        M =
                            typeof P === "symbol"
                                ? `syms[${y.indexOf(P)}]`
                                : Sr(P),
                        pt = `${M} in input`,
                        _r = h[P],
                        ft = _r?._zod?.optin,
                        yr = ft !== void 0,
                        Js = _r?._zod?.optout === "optional";
                    if ((_.write(`const ${z} = ${v(M)};`), yr && Js)) {
                        let Gs =
                            ft === "optional"
                                ? `${z}_present`
                                : `${z}.value !== undefined || ${z}_present`;
                        _.write(`
        const ${z}_present = ${pt};
        if (!${z}.issues.length || ${z}_present) {
          if (${z}.issues.length) {${L(z, M)}
          }

          if (${Gs}) {
            newResult[${M}] = ${z}.value;
          }
        }

      `);
                    } else if (!yr)
                        _.write(`
        const ${z}_present = ${pt};
        if (${z}.issues.length) {${L(z, M)}
        }
        if (!${z}_present && !${z}.issues.length) {
          payload.issues.push({
            code: "invalid_type",
            expected: "nonoptional",
            input: undefined,
            path: [${M}]
          });
          if (ctx && ctx.abortEarly) {
            payload.value = newResult;
            return payload;
          }
        }

        if (${z}_present) {
          newResult[${M}] = ${z}.value;
        }

      `);
                    else if (
                        (_.write(`
        if (${z}.issues.length) {${L(z, M)}
        }
      `),
                        ft === "defaulted")
                    )
                        _.write(`newResult[${M}] = ${z}.value;`);
                    else
                        _.write(`
        if (${z}.value !== undefined || ${pt}) {
          newResult[${M}] = ${z}.value;
        }
      `);
                }
                return (
                    _.write("payload.value = newResult;"),
                    _.write("return payload;"),
                    _.compile()
                );
            },
            s,
            c = se,
            a = !T.jitless,
            l = a && Zr.value,
            f = t.catchall,
            d;
        e._zod.parse = (h, m) => {
            d ?? (d = n.value);
            let y = h.value;
            if (!c(y))
                return (
                    h.issues.push({
                        expected: "object",
                        code: "invalid_type",
                        input: y,
                        inst: e,
                    }),
                    h
                );
            if (a && l && m?.async === !1 && m.jitless !== !0) {
                if (!s) s = i(t.shape);
                if (((h = s(h, m)), !f)) return h;
                return Io([], y, h, m, d, e, m?.abortEarly === !0);
            }
            return r(h, m);
        };
    });
function Bn(e, t, r, n) {
    for (let i of e) if (i.issues.length === 0) return ((t.value = i.value), t);
    let o = e.filter((i) => !V(i));
    if (o.length === 1) return ((t.value = o[0].value), o[0]);
    return (
        t.issues.push({
            code: "invalid_union",
            input: t.value,
            inst: r,
            errors: e.map((i) => i.issues.map((s) => Y(s, n, j()))),
        }),
        t
    );
}
var Ft = p("$ZodUnion", (e, t) => {
    (k.init(e, t),
        x(e, "optin", (n) =>
            n.def.options.some((o) => o._zod.optin === "defaulted")
                ? "defaulted"
                : n.def.options.some((o) => o._zod.optin !== void 0)
                  ? "optional"
                  : void 0,
        ),
        x(e, "optout", (n) =>
            n.def.options.some((o) => o._zod.optout === "optional")
                ? "optional"
                : void 0,
        ),
        x(e, "values", (n) => {
            if (n.def.options.every((o) => o._zod.values))
                return new Set(
                    n.def.options.flatMap((o) => Array.from(o._zod.values)),
                );
            return;
        }),
        x(e, "pattern", (n) => {
            if (n.def.options.every((o) => o._zod.pattern)) {
                let o = n.def.options.map((i) => i._zod.pattern);
                return new RegExp(
                    `^(${o.map((i) => Ee(i.source)).join("|")})$`,
                );
            }
            return;
        }));
    let r = t.options.length === 1 ? t.options[0]._zod.run : null;
    e._zod.parse = (n, o) => {
        if (r) return r(n, o);
        let i = !1,
            s = [];
        for (let c of t.options) {
            let a = c._zod.run({ value: n.value, issues: [] }, o);
            if (a instanceof Promise) (s.push(a), (i = !0));
            else {
                if (a.issues.length === 0) return a;
                s.push(a);
            }
        }
        if (!i) return Bn(s, n, e, o);
        return Promise.all(s).then((c) => Bn(c, n, e, o));
    };
});
function jc(e) {
    let t = new Map();
    for (let r of e.options) {
        let n = r._zod.propValues?.[e.discriminator];
        if (!n || n.size === 0)
            throw Error(
                `Invalid discriminated union option at index "${e.options.indexOf(r)}"`,
            );
        for (let o of n)
            if (t.has(o)) {
                if (o !== void 0)
                    throw Error(`Duplicate discriminator value "${String(o)}"`);
                t.set(o, null);
            } else t.set(o, r);
    }
    return t;
}
var Do = p("$ZodDiscriminatedUnion", (e, t) => {
        ((t.inclusive = !1), Ft.init(e, t));
        let r = e._zod.parse;
        (x(e, "propValues", (o) => {
            let i = {},
                s = 0;
            for (let c of o.def.options) {
                let a = c._zod.propValues;
                if (!a || Object.keys(a).length === 0)
                    throw Error(
                        `Invalid discriminated union option at index "${o.def.options.indexOf(c)}"`,
                    );
                if (a[o.def.discriminator]?.has(void 0)) s++;
                for (let [u, l] of Object.entries(a)) {
                    if (!Object.prototype.hasOwnProperty.call(i, u))
                        D(i, u, new Set());
                    for (let f of l) i[u].add(f);
                }
            }
            if (!o.def.unionFallback && s > 1)
                i[o.def.discriminator]?.delete(void 0);
            return i;
        }),
            t.options.forEach((o, i) => {
                let s = ye(o._zod.def);
                if (
                    s &&
                    !Object.prototype.hasOwnProperty.call(s, t.discriminator)
                )
                    throw Error(
                        `Invalid discriminated union option at index "${i}"`,
                    );
            }));
        let n = _e(() => jc(t));
        e._zod.parse = (o, i) => {
            let s = o.value;
            if (!se(s))
                return (
                    o.issues.push({
                        code: "invalid_type",
                        expected: "object",
                        input: s,
                        inst: e,
                    }),
                    o
                );
            let c = s?.[t.discriminator],
                a = n.value.get(c);
            if (a && (c !== void 0 || i.direction !== "backward"))
                return a._zod.run(o, i);
            if (t.unionFallback || i.direction === "backward") return r(o, i);
            return (
                o.issues.push({
                    code: "invalid_union",
                    errors: [],
                    note: "No matching discriminator",
                    discriminator: t.discriminator,
                    options: Array.from(n.value.keys()).filter(
                        (u) => n.value.get(u) !== null,
                    ),
                    input: s,
                    path: [t.discriminator],
                    inst: e,
                }),
                o
            );
        };
    }),
    Ro = p("$ZodIntersection", (e, t) => {
        (k.init(e, t),
            (e._zod.parse = (r, n) => {
                let o = r.value,
                    i = t.left._zod.run({ value: o, issues: [] }, n),
                    s = t.right._zod.run({ value: o, issues: [] }, n);
                if (i instanceof Promise || s instanceof Promise)
                    return Promise.all([i, s]).then(([a, u]) => Wn(r, a, u));
                return Wn(r, i, s);
            }));
    });
function Nt(e, t) {
    if (e === t) return { valid: !0, data: e };
    if (e instanceof Date && t instanceof Date && +e === +t)
        return { valid: !0, data: e };
    if (ce(e) && ce(t)) {
        let r = Object.keys(t),
            n = Object.keys(e).filter((i) => r.indexOf(i) !== -1),
            o = { ...e, ...t };
        if (Object.prototype.hasOwnProperty.call(o, "__proto__"))
            delete o.__proto__;
        for (let i of n) {
            if (i === "__proto__") continue;
            let s = Nt(e[i], t[i]);
            if (!s.valid)
                return { valid: !1, mergeErrorPath: [i, ...s.mergeErrorPath] };
            o[i] = s.data;
        }
        return { valid: !0, data: o };
    }
    if (Array.isArray(e) && Array.isArray(t)) {
        if (e.length !== t.length) return { valid: !1, mergeErrorPath: [] };
        let r = [];
        for (let n = 0; n < e.length; n++) {
            let o = e[n],
                i = t[n],
                s = Nt(o, i);
            if (!s.valid)
                return { valid: !1, mergeErrorPath: [n, ...s.mergeErrorPath] };
            r.push(s.data);
        }
        return { valid: !0, data: r };
    }
    return { valid: !1, mergeErrorPath: [] };
}
function Wn(e, t, r) {
    let n = new Map(),
        o,
        i = new Map(),
        s = (u, l) => {
            let f;
            if (u.code === "unrecognized_keys" && !u.path?.length)
                (o ?? (o = u), (f = u.keys));
            else if (
                u.code === "invalid_key" &&
                u.origin === "record" &&
                u.path?.length === 1
            ) {
                let d = String(u.path[0]);
                if (!i.has(d)) i.set(d, u);
                f = [d];
            } else return !1;
            for (let d of f) {
                if (!n.has(d)) n.set(d, {});
                n.get(d)[l] = !0;
            }
            return !0;
        };
    for (let u of t.issues) if (!s(u, "l")) e.issues.push(u);
    for (let u of r.issues) if (!s(u, "r")) e.issues.push(u);
    let c = [...n].filter(([, u]) => u.l && u.r).map(([u]) => u);
    if (c.length) {
        let u = o ? c.filter((l) => o.keys.includes(l)) : [];
        if (u.length) e.issues.push({ ...o, keys: u });
        for (let l of c)
            if (!u.includes(l) && i.has(l)) e.issues.push(i.get(l));
    }
    let a = Nt(t.value, r.value);
    if (!a.valid) {
        if (V(e)) return e;
        throw Error(
            `Unmergable intersection. Error path: ${JSON.stringify(a.mergeErrorPath)}`,
        );
    }
    return ((e.value = a.data), e);
}
var jo = p("$ZodEnum", (e, t) => {
        k.init(e, t);
        let r = me(t.entries),
            n = new Set(r);
        ((e._zod.values = n),
            x(e, "pattern", (o) => {
                let i = me(o.def.entries).filter((s) => Ar.has(typeof s));
                return new RegExp(
                    i.length
                        ? `^(${i.map((s) => X(s.toString())).join("|")})$`
                        : "^[^\\s\\S]$",
                );
            }),
            (e._zod.parse = (o, i) => {
                let s = o.value;
                if (n.has(s)) return o;
                return (
                    o.issues.push({
                        code: "invalid_value",
                        values: r,
                        input: s,
                        inst: e,
                    }),
                    o
                );
            }));
    }),
    Fo = p("$ZodLiteral", (e, t) => {
        k.init(e, t);
        let r = new Set(t.values);
        ((e._zod.values = r),
            x(e, "pattern", (n) => {
                let o = n.def.values;
                return new RegExp(
                    o.length
                        ? `^(${o.map((i) => (typeof i === "string" ? X(i) : i ? X(i.toString()) : String(i))).join("|")})$`
                        : "^[^\\s\\S]$",
                );
            }),
            (e._zod.parse = (n, o) => {
                let i = n.value;
                if (r.has(i)) return n;
                return (
                    n.issues.push({
                        code: "invalid_value",
                        values: t.values,
                        input: i,
                        inst: e,
                    }),
                    n
                );
            }));
    });
var Lo = p("$ZodTransform", (e, t) => {
    (k.init(e, t),
        (e._zod.optin = "optional"),
        T.memoizer?.guard(e),
        (e._zod.parse = (r, n) => {
            if (n.direction === "backward") throw new xe(e.constructor.name);
            let o = t.transform(r.value, r);
            if (n.async)
                return (o instanceof Promise ? o : Promise.resolve(o)).then(
                    (s) => ((r.value = s), r),
                );
            if (o instanceof Promise) throw new H();
            return ((r.value = o), r);
        }));
});
function Jn(e, t) {
    return ((e.value = t.issues.length ? void 0 : t.value), e);
}
var Lt = p("$ZodOptional", (e, t) => {
        (k.init(e, t),
            x(e, "optin", (r) =>
                r.def.innerType._zod.optin === "defaulted"
                    ? "defaulted"
                    : "optional",
            ),
            (e._zod.optout = "optional"),
            x(e, "values", (r) => {
                let n = r.def.innerType._zod.values;
                return n ? new Set([...n, void 0]) : void 0;
            }),
            x(e, "pattern", (r) => {
                let n = r.def.innerType._zod.pattern;
                return n ? new RegExp(`^(${Ee(n.source)})?$`) : void 0;
            }),
            (e._zod.parse = (r, n) => {
                if (r.value === void 0) {
                    if (t.innerType._zod.optin !== "defaulted") return r;
                    let o = t.innerType._zod.run(
                        { value: r.value, issues: [] },
                        n,
                    );
                    if (o instanceof Promise) return o.then((i) => Jn(r, i));
                    return Jn(r, o);
                }
                return t.innerType._zod.run(r, n);
            }));
    }),
    Mo = p("$ZodExactOptional", (e, t) => {
        (Lt.init(e, t),
            x(e, "values", (r) => r.def.innerType._zod.values),
            x(e, "pattern", (r) => r.def.innerType._zod.pattern),
            (e._zod.parse = (r, n) => t.innerType._zod.run(r, n)));
    }),
    Uo = p("$ZodNullable", (e, t) => {
        (k.init(e, t),
            x(e, "optin", (r) => r.def.innerType._zod.optin),
            x(e, "optout", (r) => r.def.innerType._zod.optout),
            x(e, "pattern", (r) => {
                let n = r.def.innerType._zod.pattern;
                return n ? new RegExp(`^(${Ee(n.source)}|null)$`) : void 0;
            }),
            x(e, "values", (r) =>
                r.def.innerType._zod.values
                    ? new Set([...r.def.innerType._zod.values, null])
                    : void 0,
            ),
            (e._zod.parse = (r, n) => {
                if (r.value === null) return r;
                return t.innerType._zod.run(r, n);
            }));
    }),
    Vo = p("$ZodDefault", (e, t) => {
        (k.init(e, t),
            (e._zod.optin = "defaulted"),
            x(e, "values", (r) => r.def.innerType._zod.values),
            (e._zod.parse = (r, n) => {
                if (n.direction === "backward")
                    return t.innerType._zod.run(r, n);
                if (r.value === void 0) return ((r.value = t.defaultValue), r);
                let o = t.innerType._zod.run(r, n);
                if (o instanceof Promise) return o.then((i) => Gn(i, t));
                return Gn(o, t);
            }));
    });
function Gn(e, t) {
    if (e.value === void 0) e.value = t.defaultValue;
    return e;
}
var Ho = p("$ZodPrefault", (e, t) => {
        (k.init(e, t),
            (e._zod.optin = "defaulted"),
            x(e, "values", (r) => r.def.innerType._zod.values),
            (e._zod.parse = (r, n) => {
                if (n.direction === "backward")
                    return t.innerType._zod.run(r, n);
                if (r.value === void 0) r.value = t.defaultValue;
                return t.innerType._zod.run(r, n);
            }));
    }),
    Bo = p("$ZodNonOptional", (e, t) => {
        (k.init(e, t),
            x(e, "values", (r) => {
                let n = r.def.innerType._zod.values;
                return n ? new Set([...n].filter((o) => o !== void 0)) : void 0;
            }),
            (e._zod.parse = (r, n) => {
                let o = t.innerType._zod.run(r, n);
                if (o instanceof Promise) return o.then((i) => Kn(i, e));
                return Kn(o, e);
            }));
    });
function Kn(e, t) {
    if (!e.issues.length && e.value === void 0)
        e.issues.push({
            code: "invalid_type",
            expected: "nonoptional",
            input: e.value,
            inst: t,
        });
    return e;
}
function qn(e, t, r, n) {
    if (!t.issues.length) {
        if (((e.value = t.value), t.memo)) e.memo = !0;
        return e;
    }
    return (
        (e.value = r.catchValue({
            ...t,
            value: e.value,
            error: { issues: t.issues.map((o) => Y(o, n, j())) },
            input: e.value,
        })),
        e
    );
}
var Wo = p("$ZodCatch", (e, t) => {
    (k.init(e, t),
        x(e, "optin", (r) =>
            r.def.innerType._zod.optin === "defaulted"
                ? "defaulted"
                : "optional",
        ),
        x(e, "optout", (r) => r.def.innerType._zod.optout),
        x(e, "values", (r) => r.def.innerType._zod.values),
        (e._zod.parse = (r, n) => {
            if (n.direction === "backward") return t.innerType._zod.run(r, n);
            let o = t.innerType._zod.run({ value: r.value, issues: [] }, n);
            if (o instanceof Promise) return o.then((i) => qn(r, i, t, n));
            return qn(r, o, t, n);
        }));
});
var Jo = p("$ZodPipe", (e, t) => {
    (k.init(e, t),
        x(e, "values", (r) => r.def.in._zod.values),
        x(e, "optin", (r) => r.def.in._zod.optin),
        x(e, "optout", (r) => r.def.out._zod.optout),
        x(e, "propValues", (r) => r.def.in._zod.propValues),
        (e._zod.parse = (r, n) => {
            if (n.direction === "backward") {
                let i = t.out._zod.run(r, n);
                if (i instanceof Promise) return i.then((s) => We(s, t.in, n));
                return We(i, t.in, n);
            }
            let o = t.in._zod.run(r, n);
            if (o instanceof Promise) return o.then((i) => We(i, t.out, n));
            return We(o, t.out, n);
        }));
});
function We(e, t, r) {
    if (e.issues.some((n) => n.code !== "unrecognized_keys"))
        return ((e.aborted = !0), e);
    return t._zod.run({ value: e.value, issues: e.issues }, r);
}
var Go = p("$ZodReadonly", (e, t) => {
    (k.init(e, t),
        x(e, "propValues", (r) => r.def.innerType._zod.propValues),
        x(e, "values", (r) => r.def.innerType._zod.values),
        x(e, "optin", (r) => r.def.innerType?._zod?.optin),
        x(e, "optout", (r) => r.def.innerType?._zod?.optout),
        (e._zod.parse = (r, n) => {
            if (n.direction === "backward") return t.innerType._zod.run(r, n);
            let o = t.innerType._zod.run(r, n);
            if (o instanceof Promise) return o.then(Xn);
            return Xn(o);
        }));
});
function Xn(e) {
    if (!e.memo) e.value = Object.freeze(e.value);
    return e;
}
var Ko = p("$ZodCustom", (e, t) => {
    (A.init(e, t),
        k.init(e, t),
        (e._zod.parse = (r, n) => r),
        (e._zod.check = (r) => {
            let n = r.value,
                o = t.fn(n);
            if (o instanceof Promise) return o.then((i) => Yn(i, r, n, e));
            Yn(o, r, n, e);
            return;
        }));
});
function Yn(e, t, r, n) {
    if (!e) {
        let o = {
            code: "custom",
            input: r,
            inst: n,
            path: [...(n._zod.def.path ?? [])],
            continue: !n._zod.def.abort,
        };
        if (n._zod.def.params) o.params = n._zod.def.params;
        t.issues.push(re(o));
    }
}
class Xo extends Error {
    constructor() {
        super("Cannot parse a reference cycle that closes through a transform");
        this.name = "ZodCyclicError";
    }
}
var Ut = "~memo",
    qo = [];
function Yo(e) {
    return e !== null && typeof e === "object";
}
function Mt(e) {
    return e.map((t) => (t.path ? { ...t, path: t.path.slice() } : { ...t }));
}
var Qo = new WeakMap(),
    $e = 0,
    Qe = 1,
    ze = 2;
function et(e, t, r) {
    let n = Qo.get(e);
    if (n !== void 0) return n ? ze : $e;
    if (t.has(e)) return ze;
    t.add(e);
    let o = $e,
        i = (l) => {
            if (o !== ze && l?._zod) {
                let f = et(l, t, r);
                if (f > o) o = f;
            }
        },
        s = (l, f) => {
            let d = $e;
            for (let h of Reflect.ownKeys(l)) {
                let m = Object.getOwnPropertyDescriptor(l, h);
                if (f && !m.enumerable) continue;
                let y = m.get ? Qe : m.value?._zod ? et(m.value, t, r) : $e;
                if (y > d) d = y;
            }
            return d;
        },
        c = (l) => {
            if (l > o) o = l;
        },
        a = e._zod.def,
        u = a.type;
    switch (u) {
        case "object": {
            let l = ye(a);
            (c(l ? s(l, !0) : Qe), i(a.catchall));
            break;
        }
        case "array":
            i(a.element);
            break;
        case "tuple":
            for (let l of a.items) i(l);
            i(a.rest);
            break;
        case "record":
        case "map":
            (i(a.keyType), i(a.valueType));
            break;
        case "set":
            i(a.valueType);
            break;
        case "union":
            for (let l of a.options) i(l);
            break;
        case "intersection":
            (i(a.left), i(a.right));
            break;
        case "optional":
        case "nullable":
        case "default":
        case "prefault":
        case "catch":
        case "readonly":
        case "nonoptional":
        case "promise":
        case "success":
            i(a.innerType);
            break;
        case "pipe":
            (i(a.in), i(a.out));
            break;
        case "function":
            (i(a.input), i(a.output));
            break;
        case "lazy": {
            let l = a._cachedInner ?? (r ? e._zod.innerType : void 0);
            c(l ? et(l, t, !1) : Qe);
            break;
        }
        case "template_literal":
        case "string":
        case "number":
        case "int":
        case "boolean":
        case "bigint":
        case "symbol":
        case "undefined":
        case "null":
        case "void":
        case "never":
        case "any":
        case "unknown":
        case "date":
        case "nan":
        case "enum":
        case "literal":
        case "file":
        case "transform":
        case "custom":
            break;
        default:
            for (let l in a) {
                let f = Object.getOwnPropertyDescriptor(a, l);
                if (!f || f.get) continue;
                let d = f.value;
                if (!d || typeof d !== "object") continue;
                if (d._zod) i(d);
                else if (Array.isArray(d)) for (let h of d) i(h);
            }
    }
    return (t.delete(e), Fc(e, o));
}
function Fc(e, t) {
    if (t !== Qe) Qo.set(e, t === ze);
    return t;
}
function Lc(e, t) {
    let r = e.buckets.get(t);
    if (!r) ((r = new WeakMap()), e.buckets.set(t, r));
    return r;
}
var Xe,
    Ye = [],
    Mc = {
        alloc(e, t, r) {
            let n = Xe;
            if (!n) return r;
            Xe = void 0;
            let o = { value: r, issues: null };
            return (n.set(t.value, o), Ye.push(o), r);
        },
        guard(e) {
            var t;
            ((t = e._zod).deferred ?? (t.deferred = []),
                e._zod.deferred.push(() => {
                    let r = e._zod.parse,
                        n = (o, i) => {
                            if (i.direction !== "backward" && Uc(i, o.value))
                                throw new Xo();
                            return r(o, i);
                        };
                    if (((e._zod.parse = n), e._zod.run === r)) e._zod.run = n;
                }));
        },
        attach(e) {
            var t;
            let r,
                n = !1,
                o,
                i;
            ((t = e._zod).deferred ?? (t.deferred = []),
                e._zod.deferred.push(() => {
                    let s = e._zod.parse,
                        c = (a, u) => {
                            if (r === void 0) {
                                let v = et(e, new Set(), !1);
                                if (v === $e) {
                                    if (((e._zod.parse = s), e._zod.run === c))
                                        e._zod.run = s;
                                    return s(a, u);
                                }
                                if (v === ze || n) r = !0;
                                else n = !0;
                            }
                            let l = a.value;
                            if (!Yo(l)) return s(a, u);
                            let f = u[Ut];
                            if (!f)
                                ((f = {
                                    buckets: new WeakMap(),
                                    backEdges: void 0,
                                }),
                                    (u[Ut] = f));
                            let d;
                            if (o === u) d = i;
                            else ((d = Lc(f, e)), (o = u), (i = d));
                            let h = d.get(l);
                            if (h) {
                                if (((a.value = h.value), h.issues)) {
                                    if (h.issues.length)
                                        a.issues.push(...Mt(h.issues));
                                } else
                                    ((a.memo = !0),
                                        f.backEdges ??
                                            (f.backEdges = new WeakSet()),
                                        f.backEdges.add(h.value));
                                return a;
                            }
                            Xe = d;
                            let m = Ye.length,
                                y = s(a, u);
                            Xe = void 0;
                            let _ = Ye.length > m ? Ye.pop() : void 0;
                            if (y instanceof Promise)
                                return y.then((v) => {
                                    if (_)
                                        _.issues = v.issues.length
                                            ? Mt(v.issues)
                                            : qo;
                                    return v;
                                });
                            if (_)
                                _.issues = y.issues.length ? Mt(y.issues) : qo;
                            return y;
                        };
                    if (((e._zod.parse = c), e._zod.run === s)) e._zod.run = c;
                }));
        },
    };
function ei() {
    return Mc;
}
function Uc(e, t) {
    let r = e[Ut]?.backEdges;
    return r !== void 0 && Yo(t) && r.has(t);
}
var Vc = () => {
    let e = {
        string: { unit: "characters", verb: "to have" },
        file: { unit: "bytes", verb: "to have" },
        array: { unit: "items", verb: "to have" },
        set: { unit: "items", verb: "to have" },
        map: { unit: "entries", verb: "to have" },
    };
    function t(i) {
        return e[i] ?? null;
    }
    let r = {
            regex: "input",
            email: "email address",
            url: "URL",
            emoji: "emoji",
            uuid: "UUID",
            uuidv4: "UUIDv4",
            uuidv6: "UUIDv6",
            nanoid: "nanoid",
            guid: "GUID",
            cuid: "cuid",
            cuid2: "cuid2",
            ulid: "ULID",
            xid: "XID",
            ksuid: "KSUID",
            datetime: "ISO datetime",
            date: "ISO date",
            time: "ISO time",
            duration: "ISO duration",
            ipv4: "IPv4 address",
            ipv6: "IPv6 address",
            mac: "MAC address",
            cidrv4: "IPv4 range",
            cidrv6: "IPv6 range",
            base64: "base64-encoded string",
            base64url: "base64url-encoded string",
            json_string: "JSON string",
            e164: "E.164 number",
            currency_code: "currency code",
            credit_card: "credit card number",
            iban: "IBAN",
            jwt: "JWT",
            template_literal: "input",
        },
        n = { nan: "NaN" };
    function o(i, s) {
        if (i === "number" && typeof s === "number" && !Number.isFinite(s))
            return String(s);
        return n[i] ?? i;
    }
    return (i) => {
        switch (i.code) {
            case "invalid_type": {
                let s = o(i.expected),
                    c = Nr(i.input),
                    a = o(c, i.input);
                return `Invalid input: expected ${s}, received ${a}`;
            }
            case "invalid_value":
                if (i.values.length === 1)
                    return `Invalid input: expected ${bt(i.values[0])}`;
                return `Invalid option: expected one of ${_t(i.values, "|")}`;
            case "too_big": {
                let s = i.exact ? "exactly " : i.inclusive ? "<=" : "<",
                    c = t(i.origin);
                if (c)
                    return `Too big: expected ${i.origin ?? "value"} to have ${s}${i.maximum.toString()} ${c.unit ?? "elements"}`;
                return `Too big: expected ${i.origin ?? "value"} to be ${s}${i.maximum.toString()}`;
            }
            case "too_small": {
                let s = i.exact ? "exactly " : i.inclusive ? ">=" : ">",
                    c = t(i.origin);
                if (c)
                    return `Too small: expected ${i.origin} to have ${s}${i.minimum.toString()} ${c.unit}`;
                return `Too small: expected ${i.origin} to be ${s}${i.minimum.toString()}`;
            }
            case "invalid_format": {
                let s = i;
                if (s.format === "starts_with")
                    return `Invalid string: must start with "${s.prefix}"`;
                if (s.format === "ends_with")
                    return `Invalid string: must end with "${s.suffix}"`;
                if (s.format === "includes")
                    return `Invalid string: must include "${s.includes}"`;
                if (s.format === "regex")
                    return `Invalid string: must match pattern ${s.pattern}`;
                return `Invalid ${r[s.format] ?? i.format}`;
            }
            case "not_multiple_of":
                return `Invalid number: must be a multiple of ${i.divisor}`;
            case "unrecognized_keys":
                return `Unrecognized key${i.keys.length > 1 ? "s" : ""}: ${_t(i.keys, ", ")}`;
            case "invalid_key":
                return `Invalid key in ${i.origin}`;
            case "invalid_union":
                if (
                    i.options &&
                    Array.isArray(i.options) &&
                    i.options.length > 0
                )
                    return `Invalid discriminator value. Expected ${i.options.map((c) => `'${c}'`).join(" | ")}`;
                if (i.inclusive === !1)
                    return "Invalid input: more than one option matched";
                return "Invalid input";
            case "invalid_element":
                return `Invalid value in ${i.origin}`;
            default:
                return "Invalid input";
        }
    };
};
function Vt() {
    return { localeError: Vc() };
}
var ti;
class ri {
    constructor() {
        ((this._map = new WeakMap()), (this._idmap = new Map()));
    }
    add(e, ...t) {
        let r = t[0];
        if ((this._map.set(e, r), r && typeof r === "object" && "id" in r))
            this._idmap.set(r.id, e);
        return this;
    }
    clear() {
        return ((this._map = new WeakMap()), (this._idmap = new Map()), this);
    }
    remove(e) {
        let t = this._map.get(e);
        if (t && typeof t === "object" && "id" in t) this._idmap.delete(t.id);
        return (this._map.delete(e), this);
    }
    get(e) {
        let t = e._zod.parent;
        if (t) {
            let r = { ...(this.get(t) ?? {}) };
            delete r.id;
            let n = { ...r, ...this._map.get(e) };
            return Object.keys(n).length ? n : void 0;
        }
        return this._map.get(e);
    }
    has(e) {
        return this._map.has(e);
    }
}
function Hc() {
    return new ri();
}
(ti = globalThis).__zod_globalRegistry ?? (ti.__zod_globalRegistry = Hc());
var ne = globalThis.__zod_globalRegistry;
function ni(e) {
    if (e.checks) e.checks = [...e.checks];
    return e;
}
function oi(e, t) {
    return new e(ni({ type: "string", ...g(t) }));
}
function ii(e, t) {
    return new e({
        type: "string",
        format: "email",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function si(e, t) {
    return new e({
        type: "string",
        format: "guid",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function ci(e, t) {
    return new e({
        type: "string",
        format: "uuid",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function ui(e, t) {
    return new e({
        type: "string",
        format: "uuid",
        check: "string_format",
        abort: !1,
        version: "v4",
        ...g(t),
    });
}
function ai(e, t) {
    return new e({
        type: "string",
        format: "uuid",
        check: "string_format",
        abort: !1,
        version: "v6",
        ...g(t),
    });
}
function li(e, t) {
    return new e({
        type: "string",
        format: "uuid",
        check: "string_format",
        abort: !1,
        version: "v7",
        ...g(t),
    });
}
function pi(e, t) {
    return new e({
        type: "string",
        format: "url",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function fi(e, t) {
    return new e({
        type: "string",
        format: "emoji",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function di(e, t) {
    return new e({
        type: "string",
        format: "nanoid",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function hi(e, t) {
    return new e({
        type: "string",
        format: "cuid",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function mi(e, t) {
    return new e({
        type: "string",
        format: "cuid2",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function gi(e, t) {
    return new e({
        type: "string",
        format: "ulid",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function _i(e, t) {
    return new e({
        type: "string",
        format: "xid",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function yi(e, t) {
    return new e({
        type: "string",
        format: "ksuid",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function xi(e, t) {
    return new e({
        type: "string",
        format: "ipv4",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function bi(e, t) {
    return new e({
        type: "string",
        format: "ipv6",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function vi(e, t) {
    return new e({
        type: "string",
        format: "cidrv4",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function $i(e, t) {
    return new e({
        type: "string",
        format: "cidrv6",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function zi(e, t) {
    return new e({
        type: "string",
        format: "base64",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function wi(e, t) {
    return new e({
        type: "string",
        format: "base64url",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function ki(e, t) {
    return new e({
        type: "string",
        format: "e164",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function Oi(e, t) {
    return new e({
        type: "string",
        format: "jwt",
        check: "string_format",
        abort: !1,
        ...g(t),
    });
}
function Si(e, t) {
    return new e({
        type: "string",
        format: "datetime",
        check: "string_format",
        offset: !1,
        local: !1,
        precision: null,
        ...g(t),
    });
}
function Pi(e, t) {
    return new e({
        type: "string",
        format: "date",
        check: "string_format",
        ...g(t),
    });
}
function Zi(e, t) {
    return new e({
        type: "string",
        format: "time",
        check: "string_format",
        precision: null,
        ...g(t),
    });
}
function Ai(e, t) {
    return new e({
        type: "string",
        format: "duration",
        check: "string_format",
        ...g(t),
    });
}
function Ci(e, t) {
    return new e(ni({ type: "number", checks: [], ...g(t) }));
}
function Ei(e, t) {
    return new e({
        type: "number",
        check: "number_format",
        abort: !1,
        format: "safeint",
        ...g(t),
    });
}
function Ti(e, t) {
    return new e({ type: "boolean", ...g(t) });
}
function Ii(e) {
    return new e({ type: "unknown" });
}
function Ni(e, t) {
    return new e({ type: "never", ...g(t) });
}
function tt(e, t) {
    return new Et({ check: "less_than", ...g(t), value: e, inclusive: !1 });
}
function we(e, t) {
    return new Et({ check: "less_than", ...g(t), value: e, inclusive: !0 });
}
function rt(e, t) {
    return new Tt({ check: "greater_than", ...g(t), value: e, inclusive: !1 });
}
function ke(e, t) {
    return new Tt({ check: "greater_than", ...g(t), value: e, inclusive: !0 });
}
function nt(e, t) {
    return new Zn({ check: "multiple_of", ...g(t), value: e });
}
function ot(e, t) {
    return new Cn({ check: "max_length", ...g(t), maximum: e });
}
function ue(e, t) {
    return new En({ check: "min_length", ...g(t), minimum: e });
}
function it(e, t) {
    return new Tn({ check: "length_equals", ...g(t), length: e });
}
function Ht(e, t) {
    return new In({
        check: "string_format",
        format: "regex",
        ...g(t),
        pattern: e,
    });
}
function Bt(e) {
    return new Nn({ check: "string_format", format: "lowercase", ...g(e) });
}
function Wt(e) {
    return new Dn({ check: "string_format", format: "uppercase", ...g(e) });
}
function Jt(e, t) {
    return new Rn({
        check: "string_format",
        format: "includes",
        ...g(t),
        includes: e,
    });
}
function Gt(e, t) {
    return new jn({
        check: "string_format",
        format: "starts_with",
        ...g(t),
        prefix: e,
    });
}
function Kt(e, t) {
    return new Fn({
        check: "string_format",
        format: "ends_with",
        ...g(t),
        suffix: e,
    });
}
function Q(e) {
    return new Ln({ check: "overwrite", tx: e });
}
function qt(e) {
    return Q((t) => t.normalize(e));
}
function Xt() {
    return Q((e) => e.trim());
}
function Yt() {
    return Q((e) => e.toLowerCase());
}
function Qt() {
    return Q((e) => e.toUpperCase());
}
function er() {
    return Q((e) => Pr(e));
}
function Di(e, t, r) {
    return new e({ type: "array", element: t, ...g(r) });
}
function Ri(e, t, r) {
    return new e({ type: "custom", check: "custom", fn: t, ...g(r) });
}
function ji(e, t) {
    let r = Bc(
        (n) => (
            (n.addIssue = (o) => {
                if (typeof o === "string")
                    n.issues.push(re(o, n.value, r._zod.def));
                else {
                    let i = o;
                    if (i.fatal) i.continue = !1;
                    if ((i.code ?? (i.code = "custom"), !("input" in i)))
                        i.input = n.value;
                    (i.inst ?? (i.inst = r),
                        i.continue ?? (i.continue = !r._zod.def.abort),
                        n.issues.push(re(i)));
                }
            }),
            e(n.value, n)
        ),
        t,
    );
    return r;
}
function Bc(e, t) {
    let r = new A({ check: "custom", ...g(t) });
    return ((r._zod.check = e), r);
}
function Oe(e, ...t) {
    for (let r of t)
        for (let n of Reflect.ownKeys(r))
            if (Object.prototype.propertyIsEnumerable.call(r, n)) D(e, n, r[n]);
    return e;
}
function rr(e) {
    let t = e?.target ?? "draft-2020-12";
    if (t === "draft-4") t = "draft-04";
    if (t === "draft-7") t = "draft-07";
    return {
        processors: e.processors ?? {},
        metadataRegistry: e?.metadata ?? ne,
        target: t,
        unrepresentable: e?.unrepresentable ?? "throw",
        override: e?.override ?? (() => {}),
        io: e?.io ?? "output",
        counter: 0,
        seen: new Map(),
        sharedDefsExtractedFor: void 0,
        sharedEmitDoneFor: void 0,
        cycles: e?.cycles ?? "ref",
        reused: e?.reused ?? "inline",
        intersections: [],
        deferred: [],
        external: e?.external ?? void 0,
    };
}
function G(e, t, r, n, o) {
    let i =
        typeof t.unrepresentable === "function"
            ? t.unrepresentable({ zodSchema: e, path: n.path, message: o })
            : t.unrepresentable;
    if (i === "any") return !1;
    if (i === void 0 || i === "throw") throw Error(o);
    return (Object.assign(r, i), !0);
}
function Z(e, t, r = { path: [], schemaPath: [] }) {
    var n;
    let o = e._zod.def,
        i = t.seen.get(e);
    if (i) {
        if ((i.count++, r.schemaPath.includes(e))) i.cycle = r.path;
        return i.schema;
    }
    let s = { schema: {}, count: 1, cycle: void 0, path: r.path };
    (t.seen.set(e, s),
        (t.sharedDefsExtractedFor = void 0),
        (t.sharedEmitDoneFor = void 0));
    let c = e._zod.toJSONSchema?.();
    if (c) s.schema = c;
    else {
        let l = { ...r, schemaPath: [...r.schemaPath, e], path: r.path };
        if (e._zod.processJSONSchema) e._zod.processJSONSchema(t, s.schema, l);
        else {
            let d = s.schema,
                h = t.processors[o.type];
            if (!h)
                throw Error(
                    `[toJSONSchema]: Non-representable type encountered: ${o.type}`,
                );
            h(e, t, d, l);
        }
        let f = e._zod.parent;
        if (f) {
            if (!s.ref) s.ref = f;
            (Z(f, t, l), (t.seen.get(f).isParent = !0));
        }
    }
    let a = t.metadataRegistry.get(e);
    if (a) Oe(s.schema, a);
    if (t.io === "input" && C(e))
        (delete s.schema.examples, delete s.schema.default);
    if (t.io === "input" && "_prefault" in s.schema)
        (n = s.schema).default ?? (n.default = s.schema._prefault);
    return (delete s.schema._prefault, t.seen.get(e).schema);
}
function Fi(e) {
    return e.replace(/~/g, "~0").replace(/\//g, "~1");
}
function nr(e, t) {
    let r = e.seen.get(t);
    if (!r) throw Error("Unprocessed schema. This is a bug in Zod.");
    if (e.external && e.sharedDefsExtractedFor === e.external) return;
    let n = new Map();
    for (let s of e.seen.entries()) {
        let c = e.metadataRegistry.get(s[0])?.id;
        if (c) {
            let a = n.get(c);
            if (a && a !== s[0])
                throw Error(
                    `Duplicate schema id "${c}" detected during JSON Schema conversion. Two different schemas cannot share the same id when converted together.`,
                );
            n.set(c, s[0]);
        }
    }
    let o = (s) => {
            let c = e.target === "draft-2020-12" ? "$defs" : "definitions";
            if (e.external) {
                let f = e.external.registry.get(s[0])?.id,
                    d = e.external.uri ?? ((m) => m);
                if (f) return { ref: d(f) };
                let h = s[1].defId ?? s[1].schema.id ?? `schema${e.counter++}`;
                return (
                    (s[1].defId = h),
                    { defId: h, ref: `${d("__shared")}#/${c}/${Fi(h)}` }
                );
            }
            let a = "#",
                u = `${a}/${c}/`;
            if (s[1] === r && !s[1].schema.id) return { ref: a };
            let l = s[1].schema.id ?? `__schema${e.counter++}`;
            return { defId: l, ref: u + Fi(l) };
        },
        i = (s) => {
            if (s[1].schema.$ref) return;
            let c = s[1],
                { ref: a, defId: u } = o(s);
            if (((c.def = { ...c.schema }), u)) c.defId = u;
            let l = c.schema;
            for (let f in l) delete l[f];
            l.$ref = a;
        };
    if (e.cycles === "throw")
        for (let s of e.seen.entries()) {
            let c = s[1];
            if (c.cycle)
                throw Error(`Cycle detected: #/${c.cycle?.join("/")}/<root>

Set the \`cycles\` parameter to \`"ref"\` to resolve cyclical schemas with defs.`);
        }
    for (let s of e.seen.entries()) {
        let c = s[1];
        if (t === s[0]) {
            i(s);
            continue;
        }
        if (e.external) {
            let u = e.external.registry.get(s[0])?.id;
            if (t !== s[0] && u) {
                i(s);
                continue;
            }
        }
        if (e.metadataRegistry.get(s[0])?.id) {
            i(s);
            continue;
        }
        if (c.cycle) {
            i(s);
            continue;
        }
        if (c.count > 1) {
            if (e.reused === "ref") i(s);
        }
    }
    if (e.external) e.sharedDefsExtractedFor = e.external;
}
function Ui(e) {
    let t = e.anyOf;
    if (!Array.isArray(t) || t.length === 0 || e.type !== void 0) return;
    let r = [];
    for (let n of t) {
        if (!n || typeof n !== "object") return;
        Ui(n);
        let o = Object.keys(n);
        if (o.length !== 1 || o[0] !== "type") return;
        let i = n.type;
        for (let s of Array.isArray(i) ? i : [i]) {
            if (typeof s !== "string") return;
            if (!r.includes(s)) r.push(s);
        }
    }
    (delete e.anyOf, (e.type = r.length === 1 ? r[0] : r));
}
var Vi = new Set(["type", "properties", "required", "additionalProperties"]),
    Li = ["oneOf", "anyOf"];
function Mi(e) {
    let t = e.additionalProperties;
    if (t === void 0 || t === !1 || typeof t !== "object" || t === null)
        return null;
    return Object.keys(t).length ? t : null;
}
function tr(e) {
    let t = [];
    for (let i of e) {
        if (typeof i !== "object" || i.type !== "object") return null;
        for (let s in i) if (!Vi.has(s)) return null;
        t.push(i);
    }
    let r = {},
        n = new Set();
    for (let i of t) {
        for (let s in i.properties) {
            if (Object.prototype.hasOwnProperty.call(r, s)) continue;
            let c = [];
            for (let u of t) {
                let l = u.properties?.[s] ?? Mi(u);
                if (l === null || l === void 0) continue;
                if (!c.some((f) => JSON.stringify(f) === JSON.stringify(l)))
                    c.push(l);
            }
            let a = c.length === 1 ? c[0] : (tr(c) ?? { allOf: c });
            D(r, s, a);
        }
        for (let s of i.required ?? []) n.add(s);
    }
    let o = { type: "object", properties: r };
    if (n.size) o.required = [...n];
    if (t.every((i) => i.additionalProperties === !1))
        o.additionalProperties = !1;
    else {
        let i = [];
        for (let s of t) {
            let c = Mi(s);
            if (c && !i.some((a) => JSON.stringify(a) === JSON.stringify(c)))
                i.push(c);
        }
        if (i.length === 1) o.additionalProperties = i[0];
        else if (i.length > 1) o.additionalProperties = { allOf: i };
    }
    return o;
}
function Wc(e) {
    let t = e.allOf;
    if (!Array.isArray(t) || t.length < 2) return;
    for (let o of Vi) if (o in e) return;
    let r = t.filter((o) => Li.some((i) => Array.isArray(o[i]))),
        n = null;
    if (!r.length) n = tr(t);
    else {
        let o = r[0],
            i = Li.find((a) => Array.isArray(o[a]));
        if (Object.keys(o).length !== 1) return;
        let s = t.filter((a) => a !== o),
            c = o[i].map((a) => tr([...s, a]));
        if (c.some((a) => !a)) return;
        n = { [i]: c };
    }
    if (!n) return;
    (delete e.allOf, Oe(e, n));
}
function or(e, t) {
    let r = e.seen.get(t);
    if (!r) throw Error("Unprocessed schema. This is a bug in Zod.");
    let n = (c) => {
        let a = e.seen.get(c);
        if (a.ref === null) return;
        let u = a.def ?? a.schema,
            l = { ...u },
            f = a.ref;
        if (((a.ref = null), f)) {
            n(f);
            let h = e.seen.get(f),
                m = h.schema;
            if (
                m.$ref &&
                (e.target === "draft-07" ||
                    e.target === "draft-04" ||
                    e.target === "openapi-3.0")
            )
                ((u.allOf = u.allOf ?? []), u.allOf.push(m));
            else Oe(u, m);
            if ((Oe(u, l), c._zod.parent === f))
                for (let _ in u) {
                    if (_ === "$ref" || _ === "allOf") continue;
                    if (!(_ in l)) delete u[_];
                }
            if (m.$ref && h.def)
                for (let _ in u) {
                    if (_ === "$ref" || _ === "allOf") continue;
                    if (
                        _ in h.def &&
                        JSON.stringify(u[_]) === JSON.stringify(h.def[_])
                    )
                        delete u[_];
                }
        }
        let d = c._zod.parent;
        if (d && d !== f) {
            n(d);
            let h = e.seen.get(d);
            if (h?.schema.$ref) {
                if (((u.$ref = h.schema.$ref), h.def))
                    for (let m in u) {
                        if (m === "$ref" || m === "allOf") continue;
                        if (
                            m in h.def &&
                            JSON.stringify(u[m]) === JSON.stringify(h.def[m])
                        )
                            delete u[m];
                    }
            }
        }
        e.override({ zodSchema: c, jsonSchema: u, path: a.path ?? [] });
    };
    if (!e.external || e.sharedEmitDoneFor !== e.external) {
        for (let c of [...e.seen.entries()].reverse()) n(c[0]);
        if (e.target !== "openapi-3.0")
            for (let c of e.seen.entries()) Ui(c[1].def ?? c[1].schema);
        for (let c of e.deferred) c();
        if (e.intersections.length) {
            let c = new Map();
            for (let a of e.seen.values())
                for (let u of [a.schema, a.def]) {
                    let l = u?.allOf;
                    if (!Array.isArray(l)) continue;
                    let f = c.get(l);
                    if (f) f.push(u);
                    else c.set(l, [u]);
                }
            for (let a of e.intersections) for (let u of c.get(a) ?? []) Wc(u);
        }
    }
    let o = {};
    if (e.target === "draft-2020-12")
        o.$schema = "https://json-schema.org/draft/2020-12/schema";
    else if (e.target === "draft-07")
        o.$schema = "http://json-schema.org/draft-07/schema#";
    else if (e.target === "draft-04")
        o.$schema = "http://json-schema.org/draft-04/schema#";
    else if (e.target === "openapi-3.0");
    if (e.external?.uri) {
        let c = e.external.registry.get(t)?.id;
        if (!c) throw Error("Schema is missing an `id` property");
        o.$id = e.external.uri(c);
    }
    Oe(o, r.defId ? r.schema : (r.def ?? r.schema));
    let i = e.metadataRegistry.get(t)?.id;
    if (i !== void 0 && o.id === i) delete o.id;
    let s = e.external?.defs ?? {};
    if (!e.external || e.sharedEmitDoneFor !== e.external)
        for (let c of e.seen.entries()) {
            let a = c[1];
            if (a.def && a.defId) {
                if (a.def.id === a.defId) delete a.def.id;
                D(s, a.defId, a.def);
            }
        }
    if (e.external) e.sharedEmitDoneFor = e.external;
    if (e.external);
    else if (Object.keys(s).length > 0)
        if (e.target === "draft-2020-12") o.$defs = s;
        else o.definitions = s;
    try {
        let c = JSON.parse(JSON.stringify(o));
        return (
            Object.defineProperty(c, "~standard", {
                value: {
                    ...t["~standard"],
                    jsonSchema: {
                        input: Se(t, "input", e.processors),
                        output: Se(t, "output", e.processors),
                    },
                },
                enumerable: !1,
                writable: !1,
            }),
            c
        );
    } catch (c) {
        throw Error("Error converting schema to JSON.");
    }
}
function C(e, t) {
    let r = t ?? { seen: new Set() };
    if (r.seen.has(e)) return !1;
    r.seen.add(e);
    let n = e._zod.def;
    if (n.type === "transform") return !0;
    if (n.type === "array") return C(n.element, r);
    if (n.type === "set") return C(n.valueType, r);
    if (n.type === "lazy") return C(n.getter(), r);
    if (
        n.type === "promise" ||
        n.type === "optional" ||
        n.type === "nonoptional" ||
        n.type === "nullable" ||
        n.type === "readonly" ||
        n.type === "default" ||
        n.type === "prefault" ||
        n.type === "catch"
    )
        return C(n.innerType, r);
    if (n.type === "intersection") return C(n.left, r) || C(n.right, r);
    if (n.type === "record" || n.type === "map")
        return C(n.keyType, r) || C(n.valueType, r);
    if (n.type === "pipe") {
        if (e._zod.traits.has("$ZodCodec")) return !0;
        return C(n.in, r) || C(n.out, r);
    }
    if (n.type === "object") {
        for (let o in n.shape) if (C(n.shape[o], r)) return !0;
        return !1;
    }
    if (n.type === "union") {
        for (let o of n.options) if (C(o, r)) return !0;
        return !1;
    }
    if (n.type === "tuple") {
        for (let o of n.items) if (C(o, r)) return !0;
        if (n.rest && C(n.rest, r)) return !0;
        return !1;
    }
    return !1;
}
var Hi =
        (e, t = {}) =>
        (r) => {
            let n = rr({ ...r, processors: t });
            return (Z(e, n), nr(n, e), or(n, e));
        },
    Se =
        (e, t, r = {}) =>
        (n) => {
            let { libraryOptions: o, target: i } = n ?? {},
                s = rr({ ...(o ?? {}), target: i, io: t, processors: r });
            return (Z(e, s), nr(s, e), or(s, e));
        };
var ae = (e, t, r) => {
        if (e[t] === void 0 || r > e[t]) e[t] = r;
    },
    le = (e, t, r) => {
        if (e[t] === void 0 || r < e[t]) e[t] = r;
    },
    Bi = (e, t) => {
        (ae(e, "minimum", t), le(e, "maximum", t));
    },
    Ki = (e, t) => {
        if ((e.multipleOf ?? (e.multipleOf = []), !e.multipleOf.includes(t)))
            e.multipleOf.push(t);
    },
    qi = (e, t) => {
        (e.patterns ?? (e.patterns = new Set()), e.patterns.add(t));
    },
    Xi = (e, t) => {
        e.mime = e.mime ? e.mime.filter((r) => t.includes(r)) : [...t];
    },
    Yi = (e, t) => {
        if (((e.format = t), t.includes("int"))) e.isInt = !0;
    },
    Wi = (e, t) => ae(e, "minimum", t.minimum),
    Ji = (e, t) => le(e, "maximum", t.maximum),
    Gi = (e) => (t, r) => {
        Yi(t, r.format);
        let [n, o] = e[r.format];
        (ae(t, "minimum", n), le(t, "maximum", o));
    },
    Jc = {
        greater_than: (e, t) =>
            ae(e, t.inclusive ? "minimum" : "exclusiveMinimum", t.value),
        less_than: (e, t) =>
            le(e, t.inclusive ? "maximum" : "exclusiveMaximum", t.value),
        multiple_of: (e, t) => Ki(e, t.value),
        number_format: Gi(Te),
        bigint_format: Gi(vt),
        min_length: Wi,
        max_length: Ji,
        length_equals: (e, t) => Bi(e, t.length),
        min_size: Wi,
        max_size: Ji,
        size_equals: (e, t) => Bi(e, t.size),
        string_format: (e, t) => {
            if ((Yi(e, t.format), t.pattern)) qi(e, t.pattern);
            if (t.format === "base64" || t.format === "base64url")
                e.contentEncoding = t.format;
            if (t.local || t.precision === -1) e.laxFormat = !0;
        },
        mime_type: (e, t) => Xi(e, t.mime),
    };
function F(e) {
    let t = {},
        r = e._zod.def,
        n = e._zod.traits.has("$ZodCheck")
            ? [e, ...(r.checks ?? [])]
            : (r.checks ?? []);
    for (let i of n) Jc[i._zod.def.check]?.(t, i._zod.def);
    let o = e._zod.bag;
    if (o.minimum !== void 0) ae(t, "minimum", o.minimum);
    if (o.exclusiveMinimum !== void 0)
        ae(t, "exclusiveMinimum", o.exclusiveMinimum);
    if (o.maximum !== void 0) le(t, "maximum", o.maximum);
    if (o.exclusiveMaximum !== void 0)
        le(t, "exclusiveMaximum", o.exclusiveMaximum);
    if (o.multipleOf !== void 0) Ki(t, o.multipleOf);
    if (o.format !== void 0) {
        if ((t.format ?? (t.format = o.format), o.format.includes("int")))
            t.isInt = !0;
    }
    if (o.mime) Xi(t, o.mime);
    for (let i of o.patterns ?? []) qi(t, i);
    return t;
}
var Gc = {
        guid: "uuid",
        url: "uri",
        datetime: "date-time",
        json_string: "json-string",
        regex: "",
    },
    Kc = new Map([
        [Rt, _n],
        [qe, yn],
    ]),
    qc = (e) => Kc.get(e) ?? e,
    Qi = (e, t, r, n) => {
        let o = r;
        o.type = "string";
        let {
            minimum: i,
            maximum: s,
            format: c,
            patterns: a,
            contentEncoding: u,
            laxFormat: l,
        } = F(e);
        if (typeof i === "number") o.minLength = i;
        if (typeof s === "number") o.maxLength = s;
        if (c) {
            if (((o.format = Gc[c] ?? c), o.format === "")) delete o.format;
            if (c === "time" || l) delete o.format;
        }
        if (u) o.contentEncoding = u;
        if (a && a.size > 0) {
            let f = [...a].map(qc);
            if (f.length === 1) o.pattern = f[0].source;
            else if (f.length > 1)
                o.allOf = [
                    ...f.map((d) => ({
                        ...(t.target === "draft-07" ||
                        t.target === "draft-04" ||
                        t.target === "openapi-3.0"
                            ? { type: "string" }
                            : {}),
                        pattern: d.source,
                    })),
                ];
        }
    },
    es = (e, t, r, n) => {
        let o = r,
            {
                minimum: i,
                maximum: s,
                multipleOf: c,
                exclusiveMaximum: a,
                exclusiveMinimum: u,
                isInt: l,
            } = F(e);
        o.type = l ? "integer" : "number";
        let f = typeof u === "number" && u >= (i ?? Number.NEGATIVE_INFINITY),
            d = typeof a === "number" && a <= (s ?? Number.POSITIVE_INFINITY),
            h = t.target === "draft-04" || t.target === "openapi-3.0";
        if (f)
            if (h) ((o.minimum = u), (o.exclusiveMinimum = !0));
            else o.exclusiveMinimum = u;
        else if (typeof i === "number") o.minimum = i;
        if (d)
            if (h) ((o.maximum = a), (o.exclusiveMaximum = !0));
            else o.exclusiveMaximum = a;
        else if (typeof s === "number") o.maximum = s;
        if (c) {
            let m = new Set();
            for (let v of c)
                if (Number.isFinite(v) && v !== 0) m.add(Math.abs(v));
                else
                    G(
                        e,
                        t,
                        o,
                        n,
                        `A multipleOf divisor of ${v} cannot be represented in JSON Schema`,
                    );
            let [y, ..._] = m;
            if (y !== void 0) o.multipleOf = y;
            if (_.length)
                o.allOf = [
                    ...(o.allOf ?? []),
                    ..._.map((v) => ({ multipleOf: v })),
                ];
        }
    },
    ts = (e, t, r, n) => {
        r.type = "boolean";
    };
var rs = (e, t, r, n) => {
    r.not = {};
};
var ns = (e, t, r, n) => {};
var os = (e, t, r, n) => {
        let o = e._zod.def,
            i = me(o.entries);
        if (i.length === 0) {
            r.not = {};
            return;
        }
        if (i.every((s) => typeof s === "number")) r.type = "number";
        if (i.every((s) => typeof s === "string")) r.type = "string";
        r.enum = i;
    },
    is = (e, t, r, n) => {
        let o = e._zod.def;
        if (o.values.length === 0) {
            r.not = {};
            return;
        }
        let i = [];
        for (let s of o.values)
            if (s === void 0) {
                if (
                    G(
                        e,
                        t,
                        r,
                        n,
                        "Literal `undefined` cannot be represented in JSON Schema",
                    )
                )
                    return;
            } else if (typeof s === "bigint") {
                if (
                    G(
                        e,
                        t,
                        r,
                        n,
                        "BigInt literals cannot be represented in JSON Schema",
                    )
                )
                    return;
                i.push(Number(s));
            } else i.push(s);
        if (i.length === 0);
        else if (i.length === 1) {
            let s = i[0];
            if (
                ((r.type = s === null ? "null" : typeof s),
                t.target === "draft-04" || t.target === "openapi-3.0")
            )
                r.enum = [s];
            else r.const = s;
        } else {
            if (i.every((s) => typeof s === "number")) r.type = "number";
            if (i.every((s) => typeof s === "string")) r.type = "string";
            if (i.every((s) => typeof s === "boolean")) r.type = "boolean";
            if (i.every((s) => s === null)) r.type = "null";
            r.enum = i;
        }
    };
var ss = (e, t, r, n) => {
    G(e, t, r, n, "Custom types cannot be represented in JSON Schema");
};
var cs = (e, t, r, n) => {
    G(e, t, r, n, "Transforms cannot be represented in JSON Schema");
};
var us = (e, t, r, n) => {
    let o = r,
        i = e._zod.def,
        { minimum: s, maximum: c } = F(e);
    if (typeof s === "number") o.minItems = s;
    if (typeof c === "number") o.maxItems = c;
    ((o.type = "array"),
        (o.items = Z(i.element, t, { ...n, path: [...n.path, "items"] })));
};
function ir(e) {
    let t = e._zod.def;
    if (t.type === "pipe" && t.in._zod.traits.has("$ZodTransform"))
        return ir(t.out);
    if (t.type === "catch") return ir(t.innerType);
    return e._zod.optin;
}
var as = (e, t, r, n) => {
        let o = r,
            i = e._zod.def,
            s = i.shape;
        if (
            Object.getOwnPropertySymbols(s).length &&
            G(e, t, o, n, "Symbol keys cannot be represented in JSON Schema")
        )
            return;
        ((o.type = "object"), (o.properties = {}));
        for (let u in s)
            D(
                o.properties,
                u,
                Z(s[u], t, { ...n, path: [...n.path, "properties", u] }),
            );
        let a = [];
        for (let u of Object.keys(s)) {
            let l = i.shape[u];
            if (t.io === "input" ? ir(l) === void 0 : l._zod.optout === void 0)
                a.push(u);
        }
        if (a.length > 0) o.required = a;
        if (i.catchall?._zod.def.type === "never") o.additionalProperties = !1;
        else if (!i.catchall) {
            if (t.io === "output") o.additionalProperties = !1;
        } else if (i.catchall)
            o.additionalProperties = Z(i.catchall, t, {
                ...n,
                path: [...n.path, "additionalProperties"],
            });
    },
    ls = (e, t, r, n) => {
        let o = e._zod.def,
            i = o.inclusive === !1,
            s = o.options.map((c, a) =>
                Z(c, t, { ...n, path: [...n.path, i ? "oneOf" : "anyOf", a] }),
            );
        if (i) r.oneOf = s;
        else r.anyOf = s;
    },
    ps = (e, t, r, n) => {
        let o = e._zod.def,
            i = Z(o.left, t, { ...n, path: [...n.path, "allOf", 0] }),
            s = Z(o.right, t, { ...n, path: [...n.path, "allOf", 1] }),
            c = (u) => "allOf" in u && Object.keys(u).length === 1,
            a = [...(c(i) ? i.allOf : [i]), ...(c(s) ? s.allOf : [s])];
        ((r.allOf = a), t.intersections.push(a));
    };
var fs = (e, t, r, n) => {
        let o = e._zod.def,
            i = Z(o.innerType, t, n),
            s = t.seen.get(e);
        if (t.target === "openapi-3.0")
            ((s.ref = o.innerType), (r.nullable = !0));
        else r.anyOf = [i, { type: "null" }];
    },
    ds = (e, t, r, n) => {
        let o = e._zod.def;
        Z(o.innerType, t, n);
        let i = t.seen.get(e);
        i.ref = o.innerType;
    },
    sr = Symbol();
function hs(e, t, r, n, o) {
    let i = !1,
        s = JSON.stringify(e, (c, a) => {
            if (typeof a !== "bigint") return a;
            return ((i = !0), null);
        });
    if (!i) return JSON.parse(s);
    return (
        G(t, r, n, o, "BigInt defaults cannot be represented in JSON Schema"),
        sr
    );
}
var ms = (e, t, r, n) => {
        let o = e._zod.def;
        Z(o.innerType, t, n);
        let i = t.seen.get(e);
        i.ref = o.innerType;
        let s = hs(o.defaultValue, e, t, r, n);
        if (s !== sr) r.default = s;
    },
    gs = (e, t, r, n) => {
        let o = e._zod.def;
        Z(o.innerType, t, n);
        let i = t.seen.get(e);
        if (((i.ref = o.innerType), t.io !== "input")) return;
        let s = hs(o.defaultValue, e, t, r, n);
        if (s !== sr) r._prefault = s;
    },
    _s = (e, t, r, n) => {
        let o = e._zod.def;
        Z(o.innerType, t, n);
        let i = t.seen.get(e);
        i.ref = o.innerType;
        let s;
        try {
            s = o.catchValue(void 0);
        } catch {
            G(
                e,
                t,
                r,
                n,
                "Dynamic catch values are not supported in JSON Schema",
            );
            return;
        }
        r.default = s;
    },
    ys = (e, t, r, n) => {
        let o = e._zod.def,
            i = o.in._zod.traits.has("$ZodTransform"),
            s = t.io === "input" ? (i ? o.out : o.in) : o.out;
        Z(s, t, n);
        let c = t.seen.get(e);
        c.ref = s;
    },
    xs = (e, t, r, n) => {
        let o = e._zod.def;
        Z(o.innerType, t, n);
        let i = t.seen.get(e);
        ((i.ref = o.innerType), (r.readOnly = !0));
    };
var cr = (e, t, r, n) => {
    let o = e._zod.def;
    Z(o.innerType, t, n);
    let i = t.seen.get(e);
    i.ref = o.innerType;
};
var bs = new WeakSet([Object.prototype, Error.prototype]);
function st(e, t, r) {
    Object.defineProperty(e, t, {
        configurable: !0,
        enumerable: !1,
        get() {
            let n = r(this);
            return (
                Object.defineProperty(this, t, {
                    value: n,
                    configurable: !0,
                    writable: !0,
                }),
                n
            );
        },
        set(n) {
            Object.defineProperty(this, t, {
                value: n,
                configurable: !0,
                writable: !0,
            });
        },
    });
}
var ou = (e, t) => {
    (Vr.init(e, t), (e.name = "ZodError"));
    let r = Object.getPrototypeOf(e);
    if (bs.has(r)) return;
    (bs.add(r),
        st(r, "format", (n) => (o) => Br(n, o)),
        st(r, "flatten", (n) => (o) => Hr(n, o)),
        st(r, "addIssue", (n) => (o) => {
            (n.issues.push(o), (n.message = JSON.stringify(n.issues, ge, 2)));
        }),
        st(r, "addIssues", (n) => (o) => {
            (n.issues.push(...o),
                (n.message = JSON.stringify(n.issues, ge, 2)));
        }),
        Object.defineProperty(r, "isEmpty", {
            configurable: !0,
            enumerable: !1,
            get() {
                return this.issues.length === 0;
            },
        }));
};
var I = p("ZodError", ou, void 0, { Parent: Error });
var vs = Me(I),
    $s = Ue(I),
    zs = Ve(I),
    ws = He(I),
    ks = Jr(I),
    Os = Gr(I),
    Ss = Kr(I),
    Ps = qr(I),
    Zs = Xr(I),
    As = Yr(I),
    Cs = Qr(I),
    Es = en(I);
function su() {
    if (!T.localeError) j(Vt());
}
function lr() {
    if (!T.memoizer) j({ memoizer: ei() });
}
var S = p(
        "ZodType",
        (e, t) => (su(), k.init(e, t), (e.def = t), (e.type = t.type), e),
        {
            check(...e) {
                let t = this.def;
                return this.clone(
                    E(t, {
                        checks: [
                            ...(t.checks ?? []),
                            ...e.map((r) =>
                                typeof r === "function"
                                    ? {
                                          _zod: {
                                              check: r,
                                              def: { check: "custom" },
                                              onattach: [],
                                          },
                                      }
                                    : r,
                            ),
                        ],
                    }),
                    { parent: !0 },
                );
            },
            with(...e) {
                return this.check(...e);
            },
            clone(e, t) {
                return U(this, e, t);
            },
            brand() {
                return this;
            },
            register(e, t) {
                return (e.add(this, t), this);
            },
            refine(e, t) {
                return this.check(oa(e, t));
            },
            superRefine(e, t) {
                return this.check(ia(e, t));
            },
            overwrite(e) {
                return this.check(Q(e));
            },
            optional() {
                return Ns(this);
            },
            exactOptional() {
                return Bu(this);
            },
            nullable() {
                return Ds(this);
            },
            nullish() {
                return Ns(Ds(this));
            },
            nonoptional(e) {
                return Xu(this, e);
            },
            array() {
                return fr(this);
            },
            or(e) {
                return Ru([this, e]);
            },
            and(e) {
                return Mu(this, e);
            },
            transform(e) {
                return Rs(this, Hu(e));
            },
            default(e) {
                return Gu(this, e);
            },
            prefault(e) {
                return qu(this, e);
            },
            catch(e) {
                return Qu(this, e);
            },
            pipe(e) {
                return Rs(this, e);
            },
            readonly() {
                return ra(this);
            },
            describe(e) {
                let t = this.clone();
                return (ne.add(t, { description: e }), t);
            },
            meta(...e) {
                if (e.length === 0) return ne.get(this);
                let t = this.clone();
                return (ne.add(t, e[0]), t);
            },
            isOptional() {
                return this.safeParse(void 0).success;
            },
            isNullable() {
                return this.safeParse(null).success;
            },
            apply(e, ...t) {
                return t.length === 0 ? e(this) : e(this, ...t);
            },
            get "~standard"() {
                return Fe(this, "~standard", {
                    ...Dt(this),
                    jsonSchema: {
                        input: Se(this, "input"),
                        output: Se(this, "output"),
                    },
                });
            },
            set "~standard"(e) {
                J(this, "~standard", e);
            },
            parse: function e(t, r) {
                return vs(this, t, r, { callee: e });
            },
            parseAsync: async function e(t, r) {
                return await $s(this, t, r, { callee: e });
            },
            safeParse(e, t) {
                return zs(this, e, t);
            },
            async safeParseAsync(e, t) {
                return ws(this, e, t);
            },
            get spa() {
                return this?.safeParseAsync;
            },
            set spa(e) {
                J(this, "spa", e);
            },
            validate(e, t) {
                return Ot(this, e, t);
            },
            validateAsync(e, t) {
                return St(this, e, t);
            },
            encode: function e(t, r) {
                return ks(this, t, r, { callee: e });
            },
            decode: function e(t, r) {
                return Os(this, t, r, { callee: e });
            },
            encodeAsync: async function e(t, r) {
                return await Ss(this, t, r, { callee: e });
            },
            decodeAsync: async function e(t, r) {
                return await Ps(this, t, r, { callee: e });
            },
            safeEncode(e, t) {
                return Zs(this, e, t);
            },
            safeDecode(e, t) {
                return As(this, e, t);
            },
            async safeEncodeAsync(e, t) {
                return Cs(this, e, t);
            },
            async safeDecodeAsync(e, t) {
                return Es(this, e, t);
            },
            toJSONSchema(e) {
                return Hi(this, {})(e);
            },
            get description() {
                return ne.get(this)?.description;
            },
            get _def() {
                return this._zod.def;
            },
        },
    ),
    js = p(
        "_ZodString",
        (e, t) => {
            (Ke.init(e, t),
                S.init(e, t),
                (e._zod.processJSONSchema = (r, n, o) => Qi(e, r, n, o)));
        },
        Rr(
            {
                format: (e) => F(e).format ?? null,
                minLength: (e) => F(e).minimum ?? null,
                maxLength: (e) => F(e).maximum ?? null,
            },
            {
                regex(...e) {
                    return this.check(Ht(...e));
                },
                includes(...e) {
                    return this.check(Jt(...e));
                },
                startsWith(...e) {
                    return this.check(Gt(...e));
                },
                endsWith(...e) {
                    return this.check(Kt(...e));
                },
                min(...e) {
                    return this.check(ue(...e));
                },
                max(...e) {
                    return this.check(ot(...e));
                },
                length(...e) {
                    return this.check(it(...e));
                },
                nonempty(...e) {
                    return this.check(ue(1, ...e));
                },
                lowercase(e) {
                    return this.check(Bt(e));
                },
                uppercase(e) {
                    return this.check(Wt(e));
                },
                trim() {
                    return this.check(Xt());
                },
                normalize(...e) {
                    return this.check(qt(...e));
                },
                toLowerCase() {
                    return this.check(Yt());
                },
                toUpperCase() {
                    return this.check(Qt());
                },
                slugify() {
                    return this.check(er());
                },
            },
        ),
    ),
    cu = p(
        "ZodString",
        (e, t) => {
            (Ke.init(e, t), js.init(e, t));
        },
        {
            email(e) {
                return this.check(ii(fu, e));
            },
            url(e) {
                return this.check(pi(hu, e));
            },
            jwt(e) {
                return this.check(Oi(Zu, e));
            },
            emoji(e) {
                return this.check(fi(mu, e));
            },
            guid(e) {
                return this.check(si(du, e));
            },
            uuid(e) {
                return this.check(ci(ct, e));
            },
            uuidv4(e) {
                return this.check(ui(ct, e));
            },
            uuidv6(e) {
                return this.check(ai(ct, e));
            },
            uuidv7(e) {
                return this.check(li(ct, e));
            },
            nanoid(e) {
                return this.check(di(gu, e));
            },
            cuid(e) {
                return this.check(hi(_u, e));
            },
            cuid2(e) {
                return this.check(mi(yu, e));
            },
            ulid(e) {
                return this.check(gi(xu, e));
            },
            base64(e) {
                return this.check(zi(Ou, e));
            },
            base64url(e) {
                return this.check(wi(Su, e));
            },
            xid(e) {
                return this.check(_i(bu, e));
            },
            ksuid(e) {
                return this.check(yi(vu, e));
            },
            ipv4(e) {
                return this.check(xi($u, e));
            },
            ipv6(e) {
                return this.check(bi(zu, e));
            },
            cidrv4(e) {
                return this.check(vi(wu, e));
            },
            cidrv6(e) {
                return this.check($i(ku, e));
            },
            e164(e) {
                return this.check(ki(Pu, e));
            },
            datetime(e) {
                return this.check(Si(uu, e));
            },
            date(e) {
                return this.check(Pi(au, e));
            },
            time(e) {
                return this.check(Zi(lu, e));
            },
            duration(e) {
                return this.check(Ai(pu, e));
            },
        },
    );
function N(e) {
    return oi(cu, e);
}
var O = p("ZodStringFormat", (e, t) => {
        (w.init(e, t), js.init(e, t));
    }),
    uu = p("ZodISODateTime", (e, t) => {
        (ho.init(e, t), O.init(e, t));
    }),
    au = p("ZodISODate", (e, t) => {
        (mo.init(e, t), O.init(e, t));
    }),
    lu = p("ZodISOTime", (e, t) => {
        (go.init(e, t), O.init(e, t));
    }),
    pu = p("ZodISODuration", (e, t) => {
        (_o.init(e, t), O.init(e, t));
    }),
    fu = p("ZodEmail", (e, t) => {
        (ro.init(e, t), O.init(e, t));
    });
var du = p("ZodGUID", (e, t) => {
    (eo.init(e, t), O.init(e, t));
});
var ct = p("ZodUUID", (e, t) => {
    (to.init(e, t), O.init(e, t));
});
var hu = p("ZodURL", (e, t) => {
    (io.init(e, t), O.init(e, t));
});
var mu = p("ZodEmoji", (e, t) => {
    (so.init(e, t), O.init(e, t));
});
var gu = p("ZodNanoID", (e, t) => {
    (co.init(e, t), O.init(e, t));
});
var _u = p("ZodCUID", (e, t) => {
    (uo.init(e, t), O.init(e, t));
});
var yu = p("ZodCUID2", (e, t) => {
    (ao.init(e, t), O.init(e, t));
});
var xu = p("ZodULID", (e, t) => {
    (lo.init(e, t), O.init(e, t));
});
var bu = p("ZodXID", (e, t) => {
    (po.init(e, t), O.init(e, t));
});
var vu = p("ZodKSUID", (e, t) => {
    (fo.init(e, t), O.init(e, t));
});
var $u = p("ZodIPv4", (e, t) => {
    (yo.init(e, t), O.init(e, t));
});
var zu = p("ZodIPv6", (e, t) => {
    (bo.init(e, t), O.init(e, t));
});
var wu = p("ZodCIDRv4", (e, t) => {
    (vo.init(e, t), O.init(e, t));
});
var ku = p("ZodCIDRv6", (e, t) => {
    ($o.init(e, t), O.init(e, t));
});
var Ou = p("ZodBase64", (e, t) => {
    (wo.init(e, t), O.init(e, t));
});
var Su = p("ZodBase64URL", (e, t) => {
    (ko.init(e, t), O.init(e, t));
});
var Pu = p("ZodE164", (e, t) => {
    (Oo.init(e, t), O.init(e, t));
});
var Zu = p("ZodJWT", (e, t) => {
    (So.init(e, t), O.init(e, t));
});
var Fs = p(
    "ZodNumber",
    (e, t) => {
        (jt.init(e, t),
            S.init(e, t),
            (e._zod.processJSONSchema = (r, n, o) => es(e, r, n, o)),
            (e.isFinite = !0));
    },
    Rr(
        {
            minValue: (e) => {
                let { minimum: t, exclusiveMinimum: r } = F(e);
                return Math.max(
                    t ?? Number.NEGATIVE_INFINITY,
                    r ?? Number.NEGATIVE_INFINITY,
                );
            },
            maxValue: (e) => {
                let { maximum: t, exclusiveMaximum: r } = F(e);
                return Math.min(
                    t ?? Number.POSITIVE_INFINITY,
                    r ?? Number.POSITIVE_INFINITY,
                );
            },
            isInt: (e) => {
                let { isInt: t, multipleOf: r } = F(e);
                return !!t || !!r?.some(Number.isSafeInteger);
            },
            format: (e) => F(e).format ?? null,
        },
        {
            gt(e, t) {
                return this.check(rt(e, t));
            },
            gte(e, t) {
                return this.check(ke(e, t));
            },
            min(e, t) {
                return this.check(ke(e, t));
            },
            lt(e, t) {
                return this.check(tt(e, t));
            },
            lte(e, t) {
                return this.check(we(e, t));
            },
            max(e, t) {
                return this.check(we(e, t));
            },
            int(e) {
                return this.check(Ts(e));
            },
            safe(e) {
                return this.check(Ts(e));
            },
            positive(e) {
                return this.check(rt(0, e));
            },
            nonnegative(e) {
                return this.check(ke(0, e));
            },
            negative(e) {
                return this.check(tt(0, e));
            },
            nonpositive(e) {
                return this.check(we(0, e));
            },
            multipleOf(e, t) {
                return this.check(nt(e, t));
            },
            step(e, t) {
                return this.check(nt(e, t));
            },
            finite() {
                return this;
            },
        },
    ),
);
function pr(e) {
    return Ci(Fs, e);
}
var Au = p("ZodNumberFormat", (e, t) => {
    (Po.init(e, t), Fs.init(e, t));
});
function Ts(e) {
    return Ei(Au, e);
}
var Cu = p("ZodBoolean", (e, t) => {
    (Zo.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => ts(e, r, n, o)));
});
function ut(e) {
    return Ti(Cu, e);
}
var Eu = p("ZodUnknown", (e, t) => {
    (Ao.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => ns(e, r, n, o)));
});
function Is() {
    return Ii(Eu);
}
var Tu = p("ZodNever", (e, t) => {
    (Co.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => rs(e, r, n, o)));
});
function Iu(e) {
    return Ni(Tu, e);
}
var Nu = p(
    "ZodArray",
    (e, t) => {
        (lr(),
            Eo.init(e, t),
            S.init(e, t),
            (e._zod.processJSONSchema = (r, n, o) => us(e, r, n, o)),
            (e.element = t.element));
    },
    {
        min(e, t) {
            return this.check(ue(e, t));
        },
        nonempty(e) {
            return this.check(ue(1, e));
        },
        max(e, t) {
            return this.check(ot(e, t));
        },
        length(e, t) {
            return this.check(it(e, t));
        },
        unwrap() {
            return this.element;
        },
    },
);
function fr(e, t) {
    return Di(Nu, e, t);
}
var Du = p(
    "ZodObject",
    (e, t) => {
        (lr(),
            No.init(e, t),
            S.init(e, t),
            (e._zod.processJSONSchema = (r, n, o) => as(e, r, n, o)),
            pc(e, "shape", (r) => r._zod.def.shape, !1));
    },
    {
        keyof() {
            return dr(Object.keys(this._zod.def.shape));
        },
        catchall(e) {
            return this.clone(E(this._zod.def, { catchall: e }));
        },
        passthrough() {
            return this.clone(E(this._zod.def, { catchall: Is() }));
        },
        loose() {
            return this.clone(E(this._zod.def, { catchall: Is() }));
        },
        strict() {
            return this.clone(E(this._zod.def, { catchall: Iu() }));
        },
        strip() {
            return this.clone(E(this._zod.def, { catchall: void 0 }));
        },
        extend(e) {
            return nc(this, e);
        },
        safeExtend(e) {
            return oc(this, e);
        },
        merge(e) {
            return ic(this, e);
        },
        pick(e) {
            return tc(this, e);
        },
        omit(e) {
            return rc(this, e);
        },
        partial(...e) {
            return Tr(Ms, this, e[0]);
        },
        exactPartial(...e) {
            return Tr(Us, this, e[0], "exactPartial");
        },
        required(...e) {
            return sc(Vs, this, e[0]);
        },
    },
);
function K(e, t) {
    let r = { type: "object", shape: e ?? {}, ...g(t) };
    return new Du(r);
}
var Ls = p("ZodUnion", (e, t) => {
    (Ft.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => ls(e, r, n, o)),
        (e.options = t.options));
});
function Ru(e, t) {
    return new Ls({ type: "union", options: e, ...g(t) });
}
var ju = p("ZodDiscriminatedUnion", (e, t) => {
    (Ls.init(e, t), Do.init(e, t));
});
function Fu(e, t, r) {
    return new ju({ type: "union", options: t, discriminator: e, ...g(r) });
}
var Lu = p("ZodIntersection", (e, t) => {
    (Ro.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => ps(e, r, n, o)));
});
function Mu(e, t) {
    return new Lu({ type: "intersection", left: e, right: t });
}
var ar = p("ZodEnum", (e, t) => {
    (jo.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (n, o, i) => os(e, n, o, i)),
        (e.enum = t.entries),
        (e.options = [...e._zod.values]));
    let r = new Set(Object.keys(t.entries));
    ((e.extract = (n, o) => {
        let i = {};
        for (let s of n)
            if (r.has(s)) i[s] = t.entries[s];
            else throw Error(`Key ${s} not found in enum`);
        return new ar({ ...t, checks: [], ...g(o), entries: i });
    }),
        (e.exclude = (n, o) => {
            let i = { ...t.entries };
            for (let s of n)
                if (r.has(s)) delete i[s];
                else throw Error(`Key ${s} not found in enum`);
            return new ar({ ...t, checks: [], ...g(o), entries: i });
        }));
});
function dr(e, t) {
    let r = Array.isArray(e) ? Object.fromEntries(e.map((n) => [n, n])) : e;
    return new ar({ type: "enum", entries: r, ...g(t) });
}
var Uu = p("ZodLiteral", (e, t) => {
    (Fo.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => is(e, r, n, o)),
        (e.values = new Set(t.values)),
        Object.defineProperty(e, "value", {
            get() {
                if (t.values.length > 1)
                    throw Error(
                        "This schema contains multiple valid literal values. Use `.values` instead.",
                    );
                return t.values[0];
            },
        }));
});
function hr(e, t) {
    return new Uu({
        type: "literal",
        values: Array.isArray(e) ? e : [e],
        ...g(t),
    });
}
var Vu = p("ZodTransform", (e, t) => {
    (lr(),
        Lo.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => cs(e, r, n, o)),
        (e._zod.parse = (r, n) => {
            if (n.direction === "backward") throw new xe(e.constructor.name);
            r.addIssue = (i) => {
                if (typeof i === "string") r.issues.push(re(i, r.value, t));
                else {
                    let s = i;
                    if (s.fatal) s.continue = !1;
                    if ((s.code ?? (s.code = "custom"), !("input" in s)))
                        s.input = r.value;
                    (s.inst ?? (s.inst = e), r.issues.push(re(s)));
                }
            };
            let o = t.transform(r.value, r);
            if (o instanceof Promise) return o.then((i) => ((r.value = i), r));
            return ((r.value = o), r);
        }));
});
function Hu(e) {
    return new Vu({ type: "transform", transform: e });
}
var Ms = p("ZodOptional", (e, t) => {
    (Lt.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => cr(e, r, n, o)),
        (e.unwrap = () => e._zod.def.innerType));
});
function Ns(e) {
    return new Ms({ type: "optional", innerType: e });
}
var Us = p("ZodExactOptional", (e, t) => {
    (Mo.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => cr(e, r, n, o)),
        (e.unwrap = () => e._zod.def.innerType));
});
function Bu(e) {
    return new Us({ type: "optional", innerType: e });
}
var Wu = p("ZodNullable", (e, t) => {
    (Uo.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => fs(e, r, n, o)),
        (e.unwrap = () => e._zod.def.innerType));
});
function Ds(e) {
    return new Wu({ type: "nullable", innerType: e });
}
var Ju = p("ZodDefault", (e, t) => {
    (Vo.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => ms(e, r, n, o)),
        (e.unwrap = () => e._zod.def.innerType),
        (e.removeDefault = e.unwrap));
});
function Gu(e, t) {
    return new Ju({
        type: "default",
        innerType: e,
        get defaultValue() {
            return typeof t === "function" ? t() : xt(t);
        },
    });
}
var Ku = p("ZodPrefault", (e, t) => {
    (Ho.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => gs(e, r, n, o)),
        (e.unwrap = () => e._zod.def.innerType));
});
function qu(e, t) {
    return new Ku({
        type: "prefault",
        innerType: e,
        get defaultValue() {
            return typeof t === "function" ? t() : xt(t);
        },
    });
}
var Vs = p("ZodNonOptional", (e, t) => {
    (Bo.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => ds(e, r, n, o)),
        (e.unwrap = () => e._zod.def.innerType));
});
function Xu(e, t) {
    return new Vs({ type: "nonoptional", innerType: e, ...g(t) });
}
var Yu = p("ZodCatch", (e, t) => {
    (Wo.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => _s(e, r, n, o)),
        (e.unwrap = () => e._zod.def.innerType),
        (e.removeCatch = e.unwrap));
});
function Qu(e, t) {
    return new Yu({
        type: "catch",
        innerType: e,
        catchValue: typeof t === "function" ? t : jr(t),
    });
}
var ea = p("ZodPipe", (e, t) => {
    (Jo.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => ys(e, r, n, o)),
        (e.in = t.in),
        (e.out = t.out));
});
function Rs(e, t) {
    return new ea({ type: "pipe", in: e, out: t });
}
var ta = p("ZodReadonly", (e, t) => {
    (Go.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => xs(e, r, n, o)),
        (e.unwrap = () => e._zod.def.innerType));
});
function ra(e) {
    return new ta({ type: "readonly", innerType: e });
}
var na = p("ZodCustom", (e, t) => {
    (Ko.init(e, t),
        S.init(e, t),
        (e._zod.processJSONSchema = (r, n, o) => ss(e, r, n, o)));
});
function oa(e, t = {}) {
    return Ri(na, e, t);
}
function ia(e, t) {
    return ji(e, t);
}
var ca = K({
        secs: pr()
            .int()
            .nonnegative()
            .describe("The number of whole seconds elapsed."),
        nanos: pr()
            .int()
            .nonnegative()
            .describe("The number of nanoseconds elapsed."),
    }),
    mr = K({
        type: dr(["lib", "service", "script", "app"])
            .or(N())
            .optional()
            .describe(
                "The type of project (e.g., library, service, application).",
            ),
        language: dr(["rust", "typescript"])
            .or(N())
            .optional()
            .describe("The primary language of the project."),
    }),
    gr = K({
        task_name: N().describe(
            "The short name of the task (e.g., 'test', 'build').",
        ),
        task_exec: N().nullish().describe("The command executed for the task."),
        project_name: N().describe("The name of the project."),
        project_dir: N().describe(
            "The absolute directory path of the project.",
        ),
        full_task_name: N().describe(
            "The fully qualified task name (e.g., 'omni_utils#test').",
        ),
        dependencies: fr(N()).describe("A list of dependent task names."),
        enabled: ut()
            .or(N())
            .optional()
            .describe(
                "Whether the task is enabled by configuration. Either a boolean or a tera template string that evaluates to a boolean.",
            ),
        interactive: ut().describe("Whether the task is interactive."),
        persistent: ut().describe("Whether the task is persistent."),
    }),
    ua = K({
        status: hr("completed"),
        hash: N().describe(
            "The task's content hash (Base64 encoded string). Used for caching.",
        ),
        task: gr,
        exit_code: pr()
            .int()
            .describe(
                "The exit code of the executed command (typically 0 for success).",
            ),
        elapsed: ca.describe("The duration the task took to execute."),
        cache_hit: ut().describe(
            "Indicates if the result was pulled from cache.",
        ),
        details: K({ meta: mr }).describe("Additional run details."),
    }),
    aa = K({
        status: hr("errored"),
        task: gr,
        error: N().describe("The error message."),
        details: K({ meta: mr }).describe("Additional run details."),
    }),
    la = K({
        status: hr("skipped"),
        task: gr,
        skip_reason: N().describe(
            "The reason the task was skipped (e.g., 'disabled').",
        ),
        details: K({ meta: mr }).describe("Additional run details."),
    }),
    pa = Fu("status", [ua, la, aa]).describe(
        "Schema for a single task execution result (completed or skipped).",
    ),
    Hs = fr(pa).describe("An array of task execution results.");
function at(e) {
    return {
        total: e,
        skipped: 0,
        errored: 0,
        completed: 0,
        completed_with_error: 0,
        completed_with_success: 0,
        completed_with_cache_hit: 0,
        completed_with_cache_hit_error: 0,
        completed_with_cache_hit_success: 0,
    };
}
function Bs(e) {
    let t = {
        ...at(e.length),
        aggregated_by_metadata: {},
        aggregated_by_project: {},
        aggregated_by_task: {},
        projects: {},
    };
    for (let r of e) {
        if (!t.projects[r.task.project_name])
            t.projects[r.task.project_name] = { tasks: {} };
        ((t.projects[r.task.project_name].tasks[r.task.task_name] = {
            execute: r.status === "completed" && !r.cache_hit,
            meta: r.details.meta,
        }),
            lt(r, t));
        let n = fa(r.details.meta);
        for (let o of n) {
            if (!t.aggregated_by_metadata[o])
                t.aggregated_by_metadata[o] = at(0);
            lt(r, t.aggregated_by_metadata[o]);
        }
        if (!t.aggregated_by_project[r.task.project_name])
            t.aggregated_by_project[r.task.project_name] = at(0);
        if (
            (lt(r, t.aggregated_by_project[r.task.project_name]),
            !t.aggregated_by_task[r.task.task_name])
        )
            t.aggregated_by_task[r.task.task_name] = at(0);
        lt(r, t.aggregated_by_task[r.task.task_name]);
    }
    for (let r in t.aggregated_by_metadata) {
        let n = t.aggregated_by_metadata[r];
        if (n) n.total = n.skipped + n.errored + n.completed;
    }
    for (let r in t.aggregated_by_project) {
        let n = t.aggregated_by_project[r];
        if (n) n.total = n.skipped + n.errored + n.completed;
    }
    for (let r in t.aggregated_by_task) {
        let n = t.aggregated_by_task[r];
        if (n) n.total = n.skipped + n.errored + n.completed;
    }
    return t;
}
function fa(e) {
    let t = [];
    if (e.type) t.push(`type:${e.type}`);
    if (e.language) t.push(`language:${e.language}`);
    return t;
}
function lt(e, t) {
    switch (e.status) {
        case "skipped":
            t.skipped++;
            break;
        case "errored":
            t.errored++;
            break;
        case "completed":
            if ((t.completed++, e.exit_code === 0)) t.completed_with_success++;
            else t.completed_with_error++;
            if (e.cache_hit)
                if ((t.completed_with_cache_hit++, e.exit_code === 0))
                    t.completed_with_cache_hit_success++;
                else t.completed_with_cache_hit_error++;
            break;
    }
}
var da = new ie();
await da
    .argument("<input>", "The input file to read from.")
    .option("-o, --output <output>", "The output file to write to.")
    .action(async (e, t) => {
        let r = await Ws.readFile(e, "utf-8"),
            n = JSON.parse(r),
            o = Hs.safeParse(n);
        if (o.success) {
            let i = o.data,
                s = Bs(i);
            if (t.output)
                await Ws.writeFile(t.output, JSON.stringify(s, null, 2));
            else console.log(s);
        } else (console.error(o.error), process.exit(1));
    })
    .parseAsync();
