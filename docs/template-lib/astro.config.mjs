// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// https://astro.build/config
export default defineConfig({
	integrations: [
		starlight({
			title: 'Tari Ootle Template Lib Documentation',
			social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/tari-project/tari-ootle' }],
			sidebar: [
				{
					label: 'Introduction',
					items: [{ label: 'Overview', link: '/introduction/' }],
				},
				{
					label: 'Guides',
					items: [
						{ label: 'Getting Started', link: '/guides/getting-started/' },
						{ label: 'Writing Templates', link: '/guides/writing-templates/' },
						{ label: 'Resources and Components', link: '/guides/resources-and-components/' },
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
