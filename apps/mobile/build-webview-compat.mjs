import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { build } from 'esbuild'

const output = pathToFileURL(resolve(process.argv[2]) + '/')
const web = new URL('web/', output)
const licenses = new URL('licenses/', output)
await Promise.all([mkdir(web, { recursive: true }), mkdir(licenses, { recursive: true })])

const { outputFiles, metafile } = await build({
  absWorkingDir: fileURLToPath(new URL('../../', import.meta.url)),
  entryPoints: ['apps/mobile/webview/css-compat.js'],
  bundle: true,
  format: 'iife',
  target: 'chrome104',
  minify: true,
  metafile: true,
  write: false,
})
await writeFile(new URL('css-compat.js', web), outputFiles[0].contents)

const packages = new Set(Object.keys(metafile.inputs).flatMap(path => {
  if (path.startsWith('(disabled):')) return []
  const index = path.lastIndexOf('node_modules/')
  if (index < 0) return []
  const segments = path.slice(index + 'node_modules/'.length).split('/')
  const name = segments.slice(0, segments[0].startsWith('@') ? 2 : 1).join('/')
  return path.slice(0, index) + 'node_modules/' + name
}))
const notices = await Promise.all([...packages].sort().map(async path => {
  const directory = new URL(`../../${path}/`, import.meta.url)
  const { name, version } = JSON.parse(await readFile(new URL('package.json', directory), 'utf8'))
  for (const filename of ['LICENSE', 'LICENSE.md', 'LICENSE-MIT', 'LICENSE.txt', 'LICENSE-MIT.txt', 'COPYING']) {
    try {
      return `${name} ${version}\n\n${await readFile(new URL(filename, directory), 'utf8')}`
    } catch (error) {
      if (error.code !== 'ENOENT') throw error
    }
  }
  throw new Error(`Missing license for ${name}`)
}))
await writeFile(new URL('webview-compat.txt', licenses), notices.join('\n\n'))
