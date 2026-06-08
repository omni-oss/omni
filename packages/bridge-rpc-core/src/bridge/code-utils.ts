export function getOrSet<K, V>(map: Map<K, V>, key: K, getValue: () => V): V {
    let value = map.get(key);
    if (value === undefined) {
        value = getValue();
        map.set(key, value);
        return value;
    } else {
        return value;
    }
}
