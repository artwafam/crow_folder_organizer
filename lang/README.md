# Translating Crow FO

Every language is one plain text file in this folder, named after its language code (`en.ini`, `pt-BR.ini`, `zh-Hans.ini`...). Each line is `key = text`. Lines starting with `;` are comments.

## Add a new language

1. Copy `en.ini` and rename it to your language code, for example `de.ini` or `fr.ini`.
2. Change `@name` to the name of the language written in that language, like `Deutsch`.
3. Translate the text after each `=`. Keep the keys before the `=` exactly as they are.
4. Open a pull request. The file is picked up automatically when Crow FO is built, no code changes needed.

Missing lines fall back to English, so a partial translation still works.

## Try it without building

Put your file in a `lang` folder next to `CrowFO.exe` (for example `%LOCALAPPDATA%\Programs\Crow FO\lang\de.ini`), open Crow FO settings and pick it under Language. A file there also overrides the built-in one with the same name, which is handy for fixing an existing translation.

## Tips

- Keep texts short. Menus and buttons have limited room.
- The `...` at the end of a menu entry means it opens a dialog. Keep it.
- `{}` is replaced by a name or a number. Keep it in the text.
