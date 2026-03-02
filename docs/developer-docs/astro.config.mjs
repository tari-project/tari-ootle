// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import skills from 'astro-skills';

// https://astro.build/config
export default defineConfig({
	site: 'https://tari-project.github.io',
	base: '/',
	integrations: [
		skills(),
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
						{ label: 'Getting Started', link: '/guides/getting-started/' },
						{ label: 'Setup a Wallet', link: '/guides/setup-a-wallet/' },
						{ label: 'Templates Overview', link: '/guides/template-overview/' },
						{ label: 'Building a Guessing Game', link: '/guides/build-a-guessing-game/' },
						{ label: 'Publish the Guessing Game', link: '/guides/publishing-templates/' },
						{ label: 'Play the Guessing Game', link: '/guides/play-the-guessing-game/' },
						{ label: 'Transaction Overview', link: '/guides/transaction-overview/' },
						{ label: 'Tari Cli', link: '/guides/cli/' },
						{ label: 'Resources', link: '/guides/resources/' },
						{ label: 'Authorization and Access', link: '/guides/authorization-and-access/' },
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
