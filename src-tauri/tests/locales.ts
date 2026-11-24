// Run with Bun: validate both product languages and every message's interpolation grammar.
import assert from 'node:assert/strict';
import { baseCompile } from '@intlify/message-compiler';
import english from '../../src/i18n/en-US';
import chinese from '../../src/i18n/zh-CN';

function messages(value: Record<string, unknown>, prefix = '', result = new Map<string, string>()) {
    for (const [key, child] of Object.entries(value)) {
        const path = prefix ? `${prefix}.${key}` : key;
        if (typeof child === 'string') result.set(path, child);
        else {
            assert(child && typeof child === 'object' && !Array.isArray(child), `Invalid locale node: ${path}`);
            messages(child as Record<string, unknown>, path, result);
        }
    }
    return result;
}
const en = messages(english);
const zh = messages(chinese);
assert.deepEqual([...zh.keys()].sort(), [...en.keys()].sort(), 'Product languages must define the same keys');
const errors: string[] = [];
for (const [locale, entries] of [['en-US', en], ['zh-CN', zh]] as [string, Map<string, string>][]) {
    for (const [key, value] of entries) {
        assert(value.length > 0, `Empty ${locale} message: ${key}`);
        baseCompile(value, { onError: (error) => errors.push(`${locale}:${key}: ${error.message}`) });
    }
}
assert.deepEqual(errors, [], 'All translated messages must compile');
function parameters(value: string) {
    return [...new Set([...value.matchAll(/\{([A-Za-z][A-Za-z\d_]*)\}/g)].map((match) => match[1]))].sort();
}
for (const [key, value] of en) assert.deepEqual(parameters(zh.get(key)!), parameters(value), `Interpolation mismatch: ${key}`);
console.log(`PASS: ${en.size} explicit message keys in each language; matching parameters and valid message grammar`);
