// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// https://astro.build/config
export default defineConfig({
	site: 'https://tari-project.github.io',
	base: '/tari-ootle',
	integrations: [
		starlight({
			title: 'Tari Ootle Documentation',
			social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/tari-project/tari-ootle' }],
			sidebar: [
				{
					label: 'Introduction',
					items: [
						{ label: 'Overview', link: '/introduction/' },
					],
				},
				{
					label: 'Guides',
					items: [
						{ label: 'Setup a Wallet', link: '/guides/setup-a-wallet/' },
						{ label: 'Getting Started', link: '/guides/getting-started/' },
						{ label: 'Templates Overview', link: '/guides/writing-templates/' },
						{ label: 'Building a Guessing Game', link: '/guides/guessing-game/' },
						{ label: 'Publishing Templates', link: '/guides/publishing-templates/' },
						{ label: 'Tari Cli', link: '/guides/cli/' },
						{ label: 'Resources', link: '/guides/resources/' },
						{ label: 'Authorization and Access', link: '/guides/authorization-and-access/' },
						{ label: 'Handling Events', link: '/guides/handling-events/' },
					],
				},
				{
					label: 'Reference',
					autogenerate: { directory: 'reference' },
				},
			],
		}),
	],
});
