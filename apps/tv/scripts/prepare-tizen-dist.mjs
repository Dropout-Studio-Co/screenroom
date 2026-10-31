#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { existsSync } from 'node:fs'
import { copyFile, mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const distDir = join(packageRoot, 'dist')
const configPath = join(packageRoot, 'config.xml')
const iconPath = join(packageRoot, 'icon.png')

const stageDir = join(packageRoot, 'build', 'widget')

const REQUIRED = ['index.html', 'main.js']

function fail(message) {
  console.error(`prepare-tizen-dist: ${message}`)
  process.exit(1)
}

async function main() {
  if (!existsSync(distDir)) {
    fail('dist/ does not exist — run the Vite build first (`pnpm build` does both).')
  }
  for (const name of REQUIRED) {
    if (!existsSync(join(distDir, name))) {
      fail(`dist/${name} is missing; the widget would not start.`)
    }
  }
  if (!existsSync(configPath)) {
    fail('config.xml is missing; a Tizen widget cannot be installed without it.')
  }
  if (!existsSync(iconPath)) {
    fail('icon.png is missing; the TV shows it on the app list.')
  }

  const html = await readFile(join(distDir, 'index.html'), 'utf8')
  if (!/<script[^>]+src=["']\.\/main\.js["']/.test(html)) {
    fail('dist/index.html does not load ./main.js relatively; the widget would load nothing.')
  }

  const stagedHtml = html
    .replace(/<script[^>]*\bsrc=["']\.\/main\.js["'][^>]*><\/script>/, '<script defer src="./main.js"></script>')
    .replace(/\s*<link[^>]*rel=["']modulepreload["'][^>]*>/g, '')
  if (!stagedHtml.includes('<script defer src="./main.js"></script>')) {
    fail('could not rewrite the bundle tag to a classic deferred script; the widget would load nothing.')
  }
  if (/type=["']module["']/.test(stagedHtml)) {
    fail('index.html still contains a module script; a widget cannot run one from file://.')
  }

  const absolute = [...html.matchAll(/(?:src|href)=["'](\/[^"']*)["']/g)].map((match) => match[1])
  if (absolute.length > 0) {
    fail(`dist/index.html references absolute paths (${absolute.join(', ')}); a widget has no origin.`)
  }

  const config = await readFile(configPath, 'utf8')
  const declared = /<content\s+src=["']([^"']+)["']/.exec(config)?.[1]
  if (declared !== 'index.html') {
    fail(`config.xml starts "${declared ?? 'nothing'}"; the widget needs index.html.`)
  }

  const version = JSON.parse(await readFile(join(packageRoot, 'package.json'), 'utf8')).version
  const stamped = config.replace(/(<widget[^>]*\bversion=["'])[^"']*(["'])/, `$1${version}$2`)
  if (stamped === config && !config.includes(`version="${version}"`)) {
    fail('could not find a version attribute in config.xml to stamp.')
  }

  await rm(stageDir, { recursive: true, force: true })
  await mkdir(stageDir, { recursive: true })

  await writeFile(join(stageDir, 'index.html'), stagedHtml, 'utf8')
  await copyFile(join(distDir, 'main.js'), join(stageDir, 'main.js'))
  if (existsSync(join(distDir, 'assets'))) {
    await copyTree(join(distDir, 'assets'), join(stageDir, 'assets'))
  }
  await writeFile(join(stageDir, 'config.xml'), stamped, 'utf8')
  await copyFile(iconPath, join(stageDir, 'icon.png'))

  for (const metadata of ['.tproject', 'tizen_web_project.yaml']) {
    if (existsSync(join(packageRoot, metadata))) {
      await copyFile(join(packageRoot, metadata), join(stageDir, metadata))
    }
  }

  const bundle = await readFile(join(stageDir, 'main.js'))
  const text = bundle.toString('utf8')
  if (text.includes('import.meta')) {
    fail('main.js contains import.meta, which is a parse error in a classic script.')
  }
  if (/^\s*(?:import|export)\s/m.test(text)) {
    fail('main.js contains a top-level import/export, which a classic script cannot parse.')
  }
  const digest = createHash('sha256').update(bundle).digest('hex')
  console.log(`prepare-tizen-dist: staged ${stageDir}`)
  console.log(
    'prepare-tizen-dist: package it with the tizen-sdk-skills build runner against that directory',
  )
  console.log(`prepare-tizen-dist: version ${version}, main.js ${bundle.length} bytes`)
  console.log(`prepare-tizen-dist: main.js sha256 ${digest}`)
  console.log('prepare-tizen-dist: config.xml and icon.png included; nothing else.')
}

async function copyTree(from, to) {
  await mkdir(to, { recursive: true })
  const { readdir, stat } = await import('node:fs/promises')
  for (const entry of await readdir(from)) {
    const source = join(from, entry)
    const target = join(to, entry)
    const info = await stat(source)
    if (info.isDirectory()) {
      await copyTree(source, target)
    } else {
      await copyFile(source, target)
    }
  }
}

await main()
