package club.touchtech.s5code.kotlin.design.component

import androidx.annotation.DrawableRes
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.R
import club.touchtech.s5code.kotlin.design.text.MarkdownFileIcon
import club.touchtech.s5code.kotlin.design.text.resolveMarkdownFileIcon

/** Pierre's file marks, shared with the React Native Markdown and file-tree UI. */
@Composable
fun S5FileIcon(
    path: String,
    modifier: Modifier = Modifier,
    contentDescription: String? = null,
    size: Dp = 16.dp,
) {
    Image(
        painter = painterResource(resolveMarkdownFileIcon(path).drawable),
        contentDescription = contentDescription,
        modifier = modifier.size(size),
    )
}

@get:DrawableRes
private val MarkdownFileIcon.drawable: Int
    get() =
        when (this) {
            MarkdownFileIcon.Agents -> R.drawable.pierre_agents
            MarkdownFileIcon.Astro -> R.drawable.pierre_astro
            MarkdownFileIcon.Babel -> R.drawable.pierre_babel
            MarkdownFileIcon.Bash -> R.drawable.pierre_bash
            MarkdownFileIcon.Biome -> R.drawable.pierre_biome
            MarkdownFileIcon.Bootstrap -> R.drawable.pierre_bootstrap
            MarkdownFileIcon.Browserslist -> R.drawable.pierre_browserslist
            MarkdownFileIcon.Bun -> R.drawable.pierre_bun
            MarkdownFileIcon.C -> R.drawable.pierre_c
            MarkdownFileIcon.Claude -> R.drawable.pierre_claude
            MarkdownFileIcon.Cpp -> R.drawable.pierre_c
            MarkdownFileIcon.Css -> R.drawable.pierre_css
            MarkdownFileIcon.Database -> R.drawable.pierre_database
            MarkdownFileIcon.Default -> R.drawable.pierre_default
            MarkdownFileIcon.Docker -> R.drawable.pierre_docker
            MarkdownFileIcon.Eslint -> R.drawable.pierre_eslint
            MarkdownFileIcon.Font -> R.drawable.pierre_font
            MarkdownFileIcon.Git -> R.drawable.pierre_git
            MarkdownFileIcon.Go -> R.drawable.pierre_go
            MarkdownFileIcon.Graphql -> R.drawable.pierre_graphql
            MarkdownFileIcon.Html -> R.drawable.pierre_html
            MarkdownFileIcon.Image -> R.drawable.pierre_image
            MarkdownFileIcon.Javascript -> R.drawable.pierre_javascript
            MarkdownFileIcon.Json -> R.drawable.pierre_json
            MarkdownFileIcon.Markdown -> R.drawable.pierre_markdown
            MarkdownFileIcon.Mcp -> R.drawable.pierre_mcp
            MarkdownFileIcon.Nextjs -> R.drawable.pierre_nextjs
            MarkdownFileIcon.Npm -> R.drawable.pierre_npm
            MarkdownFileIcon.Oxc -> R.drawable.pierre_oxc
            MarkdownFileIcon.Package -> R.drawable.pierre_package
            MarkdownFileIcon.Pnpm -> R.drawable.pierre_pnpm
            MarkdownFileIcon.Postcss -> R.drawable.pierre_postcss
            MarkdownFileIcon.Prettier -> R.drawable.pierre_prettier
            MarkdownFileIcon.Python -> R.drawable.pierre_python
            MarkdownFileIcon.React -> R.drawable.pierre_react
            MarkdownFileIcon.Readme -> R.drawable.pierre_readme
            MarkdownFileIcon.Ruby -> R.drawable.pierre_ruby
            MarkdownFileIcon.Rust -> R.drawable.pierre_rust
            MarkdownFileIcon.Sass -> R.drawable.pierre_sass
            MarkdownFileIcon.Stylelint -> R.drawable.pierre_stylelint
            MarkdownFileIcon.Svelte -> R.drawable.pierre_svelte
            MarkdownFileIcon.Svg -> R.drawable.pierre_svg
            MarkdownFileIcon.Svgo -> R.drawable.pierre_svgo
            MarkdownFileIcon.Swift -> R.drawable.pierre_swift
            MarkdownFileIcon.Table -> R.drawable.pierre_table
            MarkdownFileIcon.Tailwind -> R.drawable.pierre_tailwind
            MarkdownFileIcon.Terraform -> R.drawable.pierre_terraform
            MarkdownFileIcon.Text -> R.drawable.pierre_text
            MarkdownFileIcon.Tsconfig -> R.drawable.pierre_tsconfig
            MarkdownFileIcon.Typescript -> R.drawable.pierre_typescript
            MarkdownFileIcon.Vite -> R.drawable.pierre_vite
            MarkdownFileIcon.Vscode -> R.drawable.pierre_vscode
            MarkdownFileIcon.Vue -> R.drawable.pierre_vue
            MarkdownFileIcon.Wasm -> R.drawable.pierre_wasm
            MarkdownFileIcon.Webpack -> R.drawable.pierre_webpack
            MarkdownFileIcon.Yml -> R.drawable.pierre_yml
            MarkdownFileIcon.Zig -> R.drawable.pierre_zig
            MarkdownFileIcon.Zip -> R.drawable.pierre_zip
        }
