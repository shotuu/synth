/** @type {import('tailwindcss').Config} */

// ─── Synth dark theme ────────────────────────────────────────────────────────
// Synth is dark-only. Rather than rewrite every `bg-gray-50` / `text-gray-900`
// across ~70 components inherited from upstream, the raw Tailwind `gray` and
// `blue` scales are REMAPPED here:
//
//   gray  → inverted, violet-tinted dark neutrals (gray-50 is now the darkest
//           surface, gray-900 the lightest text) so upstream's light-theme
//           idioms — `bg-gray-50` pages, `text-gray-900` headings,
//           `border-gray-200` hairlines — all land correctly on dark without
//           touching each call site.
//   blue  → the Synth violet accent scale (Obsidian-style identity color).
//   red / green / amber / yellow / orange / purple / emerald → same curve:
//           50–300 are dark tints (light-theme `bg-red-50` alert washes land
//           as dark red-tinted panels), 400–700 stay Tailwind's saturated
//           mids (so `bg-red-600 text-white` buttons keep their contrast),
//           and 800–900 are light pastels (light-theme `text-red-800` on a
//           tinted wash lands as readable pastel-on-dark).
//
// Consequence for new code: keep writing colors AS IF designing for a light
// theme (`bg-white` card, `text-gray-900` heading, `bg-blue-600` primary
// button, `bg-red-50 text-red-800` alert) and the mapping does the rest.
// `bg-white` itself is overridden in globals.css (only the bg- utility;
// `text-white` stays genuinely white for text on accent buttons). A
// `bg-gray-900 text-gray-50` pairing yields a high-contrast light CTA on
// dark — the premium "inverted button" idiom.

// 50–300 dark tints and 800–900 pastels for a hue, merged with Tailwind's
// untouched 400–700 saturated mids.
const darkTintScale = (tints, mids, pastels) => ({
  50: tints[0], 100: tints[1], 200: tints[2], 300: tints[3],
  400: mids[0], 500: mids[1], 600: mids[2], 700: mids[3],
  800: pastels[0], 900: pastels[1],
});

module.exports = {
    darkMode: ['class'],
    content: [
    './src/pages/**/*.{js,ts,jsx,tsx,mdx}',
    './src/components/**/*.{js,ts,jsx,tsx,mdx}',
    './src/app/**/*.{js,ts,jsx,tsx,mdx}',
    // lib holds class-name sources too (speaker-colors.ts) — without this,
    // its arbitrary-value classes are never generated.
    './src/lib/**/*.{js,ts,jsx,tsx}',
  ],
  theme: {
  	extend: {
  		fontFamily: {
  			sans: [
  				'var(--font-source-sans-3)'
  			],
  			mono: [
  				'var(--font-jetbrains-mono)',
  				'ui-monospace',
  				'SFMono-Regular',
  				'Menlo',
  				'monospace'
  			]
  		},
  		colors: {
  			gray: {
  				50: '#17171a',   // darkest fill — page/panel washes (was near-white)
  				100: '#1e1e22',  // subtle fills, hover backgrounds
  				200: '#2b2b31',  // hairline borders
  				300: '#3a3a42',  // strong borders, disabled fills
  				400: '#6e6e7a',  // placeholder / disabled text
  				500: '#8c8c99',  // secondary text
  				600: '#a6a6b3',  // body secondary text
  				700: '#c4c4cf',  // strong text
  				800: '#dbdbe3',  // emphasized text
  				900: '#ededf2',  // primary text (was near-black)
  				950: '#f7f7fa'
  			},
  			blue: {
  				50: '#211c33',   // faint violet fill (chips, selected rows)
  				100: '#2a2347',  // active-nav / selected fills
  				200: '#3b3163',  // borders on violet fills
  				300: '#584a9e',  // muted accent
  				400: '#a18eff',  // bright accent text, icons, links on hover
  				500: '#8b6dff',  // primary accent — indicators, links, focus
  				600: '#7c5cf6',  // primary button background
  				700: '#6a48e0',  // button hover / pressed
  				800: '#cbbcff',  // pastel — light-theme "dark accent text" on tints
  				900: '#e2daff'   // lighter pastel text
  			},
  			red: darkTintScale(
  				['#291416', '#331a1c', '#48262a', '#7f3b41'],
  				['#f87171', '#ef4444', '#dc2626', '#b91c1c'],
  				['#f4a9ad', '#fbd5d7']
  			),
  			green: darkTintScale(
  				['#12241a', '#16301f', '#1f472d', '#2f6b44'],
  				['#4ade80', '#22c55e', '#16a34a', '#15803d'],
  				['#a3e8bc', '#d2f5df']
  			),
  			amber: darkTintScale(
  				['#2a2012', '#362a15', '#4d3c1d', '#755a29'],
  				['#fbbf24', '#f59e0b', '#d97706', '#b45309'],
  				['#f5d08c', '#fbe8c4']
  			),
  			yellow: darkTintScale(
  				['#292312', '#352d15', '#4b401d', '#6f5f28'],
  				['#facc15', '#eab308', '#ca8a04', '#a16207'],
  				['#f3dd8a', '#faefc3']
  			),
  			orange: darkTintScale(
  				['#2b1a10', '#372113', '#4e2f1a', '#7a4726'],
  				['#fb923c', '#f97316', '#ea580c', '#c2410c'],
  				['#f7c39a', '#fce1c9']
  			),
  			purple: darkTintScale(
  				['#221530', '#2c1b3f', '#3e2758', '#5d3a85'],
  				['#c084fc', '#a855f7', '#9333ea', '#7e22ce'],
  				['#d9b8f7', '#ecdafb']
  			),
  			emerald: darkTintScale(
  				['#10241d', '#143024', '#1c4634', '#2a684d'],
  				['#34d399', '#10b981', '#059669', '#047857'],
  				['#9fe8cb', '#cff5e6']
  			),
  			background: 'hsl(var(--background))',
  			foreground: 'hsl(var(--foreground))',
  			border: 'hsl(var(--border))',
  			input: 'hsl(var(--input))',
  			ring: 'hsl(var(--ring))',
  			primary: {
  				DEFAULT: 'hsl(var(--primary))',
  				foreground: 'hsl(var(--primary-foreground))'
  			},
  			secondary: {
  				DEFAULT: 'hsl(var(--secondary))',
  				foreground: 'hsl(var(--secondary-foreground))'
  			},
  			tertiary: '#8c8c99',
  			card: {
  				DEFAULT: 'hsl(var(--card))',
  				foreground: 'hsl(var(--card-foreground))'
  			},
  			popover: {
  				DEFAULT: 'hsl(var(--popover))',
  				foreground: 'hsl(var(--popover-foreground))'
  			},
  			muted: {
  				DEFAULT: 'hsl(var(--muted))',
  				foreground: 'hsl(var(--muted-foreground))'
  			},
  			accent: {
  				DEFAULT: 'hsl(var(--accent))',
  				foreground: 'hsl(var(--accent-foreground))'
  			},
  			destructive: {
  				DEFAULT: 'hsl(var(--destructive))',
  				foreground: 'hsl(var(--destructive-foreground))'
  			},
  			chart: {
  				'1': 'hsl(var(--chart-1))',
  				'2': 'hsl(var(--chart-2))',
  				'3': 'hsl(var(--chart-3))',
  				'4': 'hsl(var(--chart-4))',
  				'5': 'hsl(var(--chart-5))'
  			}
  		},
  		borderRadius: {
  			lg: 'var(--radius)',
  			md: 'calc(var(--radius) - 2px)',
  			sm: 'calc(var(--radius) - 4px)'
  		},
  		keyframes: {
  			'accordion-down': {
  				from: {
  					height: '0'
  				},
  				to: {
  					height: 'var(--radix-accordion-content-height)'
  				}
  			},
  			'accordion-up': {
  				from: {
  					height: 'var(--radix-accordion-content-height)'
  				},
  				to: {
  					height: '0'
  				}
  			}
  		},
  		animation: {
  			'accordion-down': 'accordion-down 0.2s ease-out',
  			'accordion-up': 'accordion-up 0.2s ease-out'
  		}
  	}
  },
  plugins: [require("tailwindcss-animate"), require('@tailwindcss/typography')],
}
