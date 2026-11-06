/** The part of the Markdown processor's data that its parser reads its extensions from. */
interface ParserData {
  micromarkExtensions?: unknown[];
}

/**
 * A person's own words keep every backslash they typed. Markdown reads a backslash before
 * punctuation as an escape and one at the end of a line as a line break, and drops it either way,
 * so a Windows path such as `C:\Work\app\.cache` showed as `C:\Work\app.cache`. People type
 * paths far more often than escapes, so in their words both readings are turned off; everything
 * else stays Markdown. The model's answers keep both: a model escapes on purpose.
 */
export function remarkTypedBackslashes(this: { data(): ParserData }): void {
  const data = this.data();
  const extensions = (data.micromarkExtensions ??= []);
  extensions.push({ disable: { null: ['characterEscape', 'hardBreakEscape'] } });
}
