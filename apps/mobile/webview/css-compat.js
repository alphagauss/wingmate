import postcss from 'postcss'
import hasPseudo from 'css-has-pseudo'
import selectorParser from 'postcss-selector-parser'
import valueParser from 'postcss-value-parser'

const hasSelector = CSS.supports('selector(:has(*))')
const hasContainers = CSS.supports('container-type: inline-size')
const hasMix = CSS.supports('color: color-mix(in srgb, red, blue)')
const hasLightDark = CSS.supports('color: light-dark(white, black)')
const hasScope = 'CSSScopeRule' in window
const hasDynamicViewport = CSS.supports('height: 100dvh')
const hasFilteredNth = CSS.supports('selector(:nth-last-child(2 of *))')
const hasStartingStyle = 'CSSStartingStyleRule' in window
const hasScrollbarWidth = CSS.supports('scrollbar-width: none')
const needsCss = !hasSelector || !hasContainers || !hasMix || !hasLightDark || !hasScope
  || !hasDynamicViewport || !hasFilteredNth || !hasStartingStyle || !hasScrollbarWidth

if (window.top === window && needsCss) {
  start().catch(error => console.warn('Wingmate stylesheet compatibility:', error))
}

async function start() {
  // Install the CSS transforms before the container polyfill parses the same styles.
  if (document.readyState === 'loading') {
    await new Promise(resolve => document.addEventListener('DOMContentLoaded', resolve, { once: true }))
  }
  if (!hasSelector) await import('css-has-pseudo/browser')
  const sheets = new WeakMap()
  const palettes = new Map()
  const recipes = new Map()
  const filteredChildren = new Map()
  const hasRules = new Map()
  const selectorAttributes = new Set(['class', 'id', 'disabled'])
  const trajectoryPanes = new Map()
  const trajectoryResize = !hasContainers && new ResizeObserver(() => scheduleLayout())
  const colorStyle = document.createElement('style')
  const canvas = document.createElement('canvas').getContext('2d')
  const darkScheme = matchMedia('(prefers-color-scheme: dark)')
  let pendingColors = false
  let colorId = 0
  let pendingLayout = false
  const syncProperty = (element, property, value) => {
    if (element.style.getPropertyValue(property) !== value) element.style.setProperty(property, value)
  }

  function resolveColor(nodes, palette, dark, seen = new Set()) {
    if (!nodes) return null
    let valid = true
    const parsed = valueParser(valueParser.stringify(nodes))
    parsed.walk(node => {
      if (node.type !== 'function') return
      const args = argumentsOf(node.nodes)
      if (node.value === 'var') {
        const name = valueParser.stringify(args[0]).trim()
        const raw = seen.has(name) ? '' : recipes.get(name) ?? palette.getPropertyValue(name).trim()
        let color = raw ? resolveColor(valueParser(raw).nodes, palette, dark, new Set([...seen, name]))
          : args[1] && resolveColor(args[1], palette, dark, seen)
        if (!color && name === '--sidebar-right-flash-color') {
          color = resolveColor(valueParser(dark ? 'var(--dsw-static-neutral-00)'
            : 'var(--dsw-alias-brand-primary-new-colorprimary-new-color)').nodes, palette, dark, seen)
        }
        if (!color) valid = false
        node.type = 'word'
        node.value = color ? rgba(color) : 'transparent'
        return false
      }
      if (node.value === 'light-dark') {
        const color = resolveColor(args[dark ? 1 : 0], palette, dark, seen)
        if (!color) valid = false
        node.type = 'word'
        node.value = color ? rgba(color) : 'transparent'
        return false
      }
      if (node.value === 'color-mix') {
        const color = mixColors(args, (part) => resolveColor(part, palette, dark, seen))
        if (!color) valid = false
        node.type = 'word'
        node.value = color ? rgba(color) : 'transparent'
        return false
      }
    })
    if (!valid) return null
    const css = parsed.toString().trim()
    if (!CSS.supports('color', css)) return null
    canvas.fillStyle = css
    const color = canvas.fillStyle
    if (color.startsWith('#')) return [1, 3, 5].map(i => parseInt(color.slice(i, i + 2), 16)).concat(1)
    if (color.startsWith('rgb')) return color.match(/[\d.]+/g)?.map(Number) ?? null
    return null
  }

  function refreshColors() {
    pendingColors = false
    if (!palettes.size || !document.body) return
    const dark = document.body.hasAttribute('data-ds-dark-theme')
      || getComputedStyle(document.documentElement).colorScheme === 'dark'
    const palette = getComputedStyle(document.body)
    const values = [...palettes].map(([recipe, name]) => {
      const color = resolveColor(valueParser(recipe).nodes, palette, dark)
      return color ? name + ':' + rgba(color) : ''
    })
    const css = ':root{' + values.filter(Boolean).join(';') + '}'
    if (colorStyle.textContent !== css) colorStyle.textContent = css
  }

  function scheduleColors() {
    if (!pendingColors && palettes.size) {
      pendingColors = true
      requestAnimationFrame(refreshColors)
    }
  }

  const hasSelectorPattern = /csstools-has-[0-9a-z-]+/g
  const needsTransform = /color-mix\(|light-dark\(|:has\(|\d+[dls]v(?:[hwbi]|min|max)\b|@scope\s*\(|@starting-style\b|:nth-(?:last-)?child\([^)]*\bof\s|--trajectory-tool-request-width|scrollbar-width\s*:\s*none/

  function trackSelectorAttributes(selector) {
    selectorParser().astSync(selector).walkAttributes(node => {
      selectorAttributes.add(node.attribute)
    })
  }

  function transform(css, baseURL) {
    const root = postcss.parse(css)
    root.walkDecls(decl => {
      const value = valueParser(decl.value)
      value.walk(node => {
        if (baseURL && node.type === 'function' && node.value === 'url' && node.nodes[0]) {
          const url = node.nodes[0]
          url.value = new URL(url.value, baseURL).href
          return false
        }
        if (node.type === 'function' && ((!hasMix && node.value === 'color-mix')
          || (!hasLightDark && node.value === 'light-dark'))) {
          const recipe = valueParser.stringify(node)
          node.type = 'word'
          if (!palettes.has(recipe)) {
            const name = '--wingmate-color-' + ++colorId
            palettes.set(recipe, name)
            recipes.set(name, recipe)
          }
          node.value = 'var(' + palettes.get(recipe) + ')'
          return false
        }
        if (!hasDynamicViewport && node.type === 'word' && /^-?[\d.]+[dls]v(?:[hwbi]|min|max)$/.test(node.value)) {
          node.value = node.value.replace(/([\d.])[dls]v/, '$1v').replace(/vi$/, 'vw').replace(/vb$/, 'vh')
        }
      })
      decl.value = value.toString()
    })
    root.walkAtRules(rule => {
      if (!hasDynamicViewport && rule.name === 'supports' && /^\(height:\s*100dvh\)$/.test(rule.params)) {
        rule.replaceWith(...rule.nodes)
      } else if (!hasStartingStyle && rule.name === 'starting-style') {
        rule.walkRules(child => {
          const from = postcss.rule({ selector: 'from' })
          child.walkDecls(decl => from.append(decl.clone()))
          const name = 'wingmate-entry-' + from.toString().replace(/\W/g, '').slice(0, 35)
          const animation = postcss.atRule({ name: 'keyframes', params: name }).append(from)
          const transform = from.nodes.some(decl => decl.prop === 'transform')
          const duration = transform ? 'var(--ds-transition-duration-slow) var(--ds-ease-in-out)'
            : 'var(--dsh-hover-preview-fade) linear'
          rule.before(animation)
          rule.before(postcss.atRule({ name: 'media', params: '(prefers-reduced-motion: no-preference)' })
            .append(child.clone({ nodes: [] }).append({ prop: 'animation', value: name + ' ' + duration })))
        })
        rule.remove()
      } else if (!hasScope && rule.name === 'scope' && rule.params === '([data-excel-preview])') {
        rule.walkRules(child => {
          if (child.parent.type === 'atrule' && /keyframes$/.test(child.parent.name)) return
          const prefix = ':where([data-excel-preview])'
          child.selector = selectorParser(selectors => {
            for (const selector of selectors.nodes) {
              const scopeRoot = selectorParser().astSync(prefix).first.first
              const scope = selector.nodes.find(node => node.type === 'pseudo' && node.value === ':scope')
              if (scope) scope.replaceWith(scopeRoot)
              else {
                selector.prepend(selectorParser.combinator({ value: ' ' }))
                selector.prepend(scopeRoot)
              }
            }
          }).processSync(child.selector)
        })
        rule.replaceWith(...rule.nodes)
      }
    })
    if (!hasScrollbarWidth) root.walkRules(rule => {
      if (!rule.nodes.some(node => node.type === 'decl' && node.prop === 'scrollbar-width' && node.value === 'none')) return
      rule.after(postcss.rule({ selector: rule.selectors.map(selector => selector + '::-webkit-scrollbar').join(',') })
        .append({ prop: 'display', value: 'none' }))
    })
    if (!hasFilteredNth) root.walkRules(rule => {
      if (!/:nth-(?:last-)?child\([^)]*\bof\s/.test(rule.selector)) return
      rule.selector = selectorParser(selectors => {
        selectors.walkPseudos(pseudo => {
          if (![':nth-child', ':nth-last-child'].includes(pseudo.value)) return
          const match = pseudo.toString().match(/^:nth-(last-)?child\(\s*(\d+)\s+of\s+([\s\S]+)\)$/)
          if (!match) return
          const [, last, index, filter] = match
          const key = JSON.stringify([Boolean(last), Number(index), filter])
          if (!filteredChildren.has(key)) {
            trackSelectorAttributes(filter)
            filteredChildren.set(key, {
              attribute: 'data-wingmate-nth-' + (filteredChildren.size + 1),
              last, index: Number(index), filter, marked: new Set(),
            })
          }
          const { attribute } = filteredChildren.get(key)
          // The marker already represents the filtered sibling. Keep its class-level specificity.
          const selector = selectorParser().astSync('[' + attribute + ']').first
          pseudo.replaceWith(...selector.nodes)
        })
      }).processSync(rule.selector)
    })
    if (!hasContainers) root.walkDecls(decl => {
      decl.value = decl.value.replace(/var\(--trajectory-tool-request-width,\s*calc\(36cqw - 56px\)\)/g,
        'var(--wingmate-trajectory-request-width,calc(36cqw - 56px))')
    })
    if (!hasSelector) {
      postcss([hasPseudo({ preserve: false })]).process(root, { from: undefined }).sync()
      root.walkRules(rule => {
        const markers = [...rule.selector.matchAll(hasSelectorPattern)]
        if (!markers.length) return
        // This sheet is already gated by CSS.supports. A descendant prefix
        // would exclude matches on the document element itself.
        rule.selectors = rule.selectors.map(selector => selector.replace(/^\.js-has-pseudo\s+/, ''))
        for (const [attribute] of markers) {
          if (!hasRules.has(attribute)) {
            const selector = attribute.slice('csstools-has-'.length).split('-')
              .map(value => String.fromCharCode(parseInt(value, 36))).join('')
            trackSelectorAttributes(selector)
            hasRules.set(attribute, { selector, marked: new Set() })
          }
        }
      })
    }
    return root.toString()
  }

  function applyStyle(style) {
    if (style === colorStyle || sheets.get(style) === style.textContent) return
    const source = style.textContent
    if (!source) return
    sheets.set(style, source)
    // The container polyfill rewrites its sheets after our observer. It already
    // received the converted source, and replacing it again loses its conditions.
    if (!hasContainers && /--cq-container-(?:name|type)-[a-f0-9]+:|\[data-cqc-[a-f0-9]+[~\]]/.test(source)) return
    if (!needsTransform.test(source)) return
    try {
      const converted = transform(source)
      sheets.set(style, converted)
      if (source !== converted) style.textContent = converted
      scheduleColors()
    } catch (error) {
      console.warn('Wingmate stylesheet compatibility:', error)
    } finally {
      scheduleLayout()
    }
  }

  async function applyLink(link) {
    const url = new URL(link.href)
    if (url.origin !== location.origin || !/^\/assets\/index-[\w-]+\.css$/.test(url.pathname)) return
    try {
      const response = await fetch(url)
      if (!response.ok) return
      const style = document.createElement('style')
      style.media = link.media
      style.textContent = transform(await response.text(), url)
      sheets.set(style, style.textContent)
      link.replaceWith(style)
      scheduleColors()
    } catch (error) {
      console.warn('Wingmate stylesheet compatibility:', error)
    }
  }

  function updateLayout() {
    pendingLayout = false
    for (const rule of filteredChildren.values()) {
      const siblings = new Map()
      for (const element of document.querySelectorAll(rule.filter)) {
        const parent = element.parentElement
        if (!parent) continue
        if (!siblings.has(parent)) siblings.set(parent, [])
        siblings.get(parent).push(element)
      }
      const next = new Set()
      for (const group of siblings.values()) {
        const match = group.at(rule.last ? -rule.index : rule.index - 1)
        if (match) next.add(match)
      }
      for (const element of rule.marked) if (!next.has(element)) element.removeAttribute(rule.attribute)
      for (const element of next) if (!rule.marked.has(element)) element.setAttribute(rule.attribute, '')
      rule.marked = next
    }
    for (const [attribute, rule] of hasRules) {
      let next
      try {
        next = new Set(document.querySelectorAll(rule.selector))
      } catch (error) {
        console.warn('Wingmate selector compatibility:', error)
        hasRules.delete(attribute)
        continue
      }
      for (const element of rule.marked) if (!next.has(element)) element.removeAttribute(attribute)
      for (const element of next) if (!rule.marked.has(element)) element.setAttribute(attribute, '')
      rule.marked = next
    }
    if (!hasContainers) {
      for (const [element, pane] of trajectoryPanes) {
        if (element.isConnected && pane.isConnected && element.style.getPropertyValue('--trajectory-tool-request-width')) continue
        trajectoryResize.unobserve(pane)
        trajectoryPanes.delete(element)
        element.style.removeProperty('--wingmate-trajectory-request-width')
      }
      for (const element of document.querySelectorAll('[style*="--trajectory-tool-request-width"]')) {
        const value = element.style.getPropertyValue('--trajectory-tool-request-width')
        const offset = value.match(/^calc\(58cqw\s*-\s*(-?[\d.]+px)\)$/)?.[1]
        const pane = element.querySelector('[data-trajectory-scroll]')
        if (!offset || !pane) continue
        if (!trajectoryPanes.has(element)) {
          trajectoryPanes.set(element, pane)
          trajectoryResize.observe(pane)
        }
        syncProperty(element, '--wingmate-trajectory-request-width', `calc(${pane.clientWidth * 0.58}px - ${offset})`)
      }
    }
  }

  function scheduleLayout() {
    if (!pendingLayout && (filteredChildren.size || hasRules.size || !hasContainers)) {
      pendingLayout = true
      requestAnimationFrame(updateLayout)
    }
  }

  const observer = new MutationObserver(records => {
    let layoutChanged = false
    for (const { target, addedNodes, type, attributeName } of records) {
      if (target === colorStyle) continue
      if (attributeName === 'data-ds-dark-theme' || attributeName === 'data-ds-theme-source') scheduleColors()
      if (type !== 'attributes' || selectorAttributes.has(attributeName)
        || (attributeName === 'style' && !hasContainers && (trajectoryPanes.has(target)
          || target.style?.getPropertyValue('--trajectory-tool-request-width').includes('cqw')))) {
        layoutChanged = true
      }
      const styleTarget = type === 'characterData' ? target.parentElement : target
      if (styleTarget instanceof HTMLStyleElement) applyStyle(styleTarget)
      for (const node of addedNodes) {
        if (node instanceof HTMLStyleElement) applyStyle(node)
        else if (node instanceof Element && node.childElementCount) node.querySelectorAll('style').forEach(applyStyle)
      }
    }
    if (layoutChanged) scheduleLayout()
  })
  observer.observe(document.documentElement, {
    subtree: true, childList: true, attributes: true, characterData: true,
  })
  document.head.append(colorStyle)
  darkScheme.addEventListener('change', scheduleColors)
  document.querySelectorAll('style').forEach(applyStyle)
  await Promise.all([...document.querySelectorAll('link[rel=stylesheet]')].map(applyLink))
  scheduleColors()
  if (!hasContainers) await import('container-query-polyfill')
  if (!hasSelector) {
    for (const event of ['focusin', 'focusout', 'pointerover', 'pointerout', 'input', 'change']) {
      document.addEventListener(event, scheduleLayout, true)
    }
  }
  scheduleLayout()
}

function argumentsOf(nodes) {
  const args = [[]]
  for (const node of nodes) {
    if (node.type === 'div' && node.value === ',') args.push([])
    else args.at(-1).push(node)
  }
  return args
}

function rgba([r, g, b, alpha = 1]) {
  return `rgba(${r}, ${g}, ${b}, ${alpha})`
}

function mixColors([space, left, right], resolve) {
  if (!left || !right || valueParser.stringify(space).trim() !== 'in srgb') return null
  function stop(nodes) {
    const words = nodes.filter(node => node.type !== 'space')
    const last = words.at(-1)
    const weight = last?.type === 'word' && last.value.endsWith('%') ? parseFloat(words.pop().value) / 100 : null
    return [resolve(words), weight]
  }
  const [first, firstWeight] = stop(left)
  const [second, secondWeight] = stop(right)
  if (!first || !second) return null
  const a = firstWeight ?? (secondWeight === null ? 0.5 : 1 - secondWeight)
  const b = secondWeight ?? (firstWeight === null ? 0.5 : 1 - firstWeight)
  const alphaA = (first[3] ?? 1) * a
  const alphaB = (second[3] ?? 1) * b
  const alpha = alphaA + alphaB
  if (alpha === 0) return [0, 0, 0, 0]
  return first.slice(0, 3).map((channel, i) => (channel * alphaA + second[i] * alphaB) / alpha)
    .concat(alpha / Math.max(1, a + b))
}
