export function replaceGroup(
    str: string,
    regex: RegExp,
    groupName: string,
    newValue: string,
) {
    if (!regex.test(str)) throw new RegexNotMatchedError(regex.source);

    return str.replace(regex, (match, ...args) => {
        // The last argument is the groups object
        const groups = args[args.length - 1];
        const fullMatch = match;
        const groupValue = groups[groupName];

        if (groupValue === undefined) return fullMatch;

        // Find the position of the group value within the full match
        // and swap it out for the new value
        return fullMatch.replace(groupValue, newValue);
    });
}

export class RegexNotMatchedError extends Error {
    constructor(public readonly pattern: string) {
        super(`Regex ${pattern} did not match`);
        super.name = this.constructor.name;
    }
}
