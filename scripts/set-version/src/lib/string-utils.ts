export function replaceGroup(
    str: string,
    regex: RegExp,
    groupName: string,
    newValue: string,
) {
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
