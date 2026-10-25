import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import useBaseUrl from '@docusaurus/useBaseUrl';
import Layout from '@theme/Layout';
import clsx from 'clsx';
import styles from './index.module.css';

function HomepageHeader() {
  const {siteConfig} = useDocusaurusContext();
  const logoUrl = useBaseUrl('/img/wax-full-logo.png');
  const repositoryUrl = `https://github.com/${siteConfig.organizationName}/${siteConfig.projectName}`;

  return (
    <header className={styles.hero}>
      <div className={clsx('container', styles.heroContent)}>
        <img
          className={styles.logo}
          src={logoUrl}
          alt="Wax: design system coverage analytics"
        />
        <p className={styles.tagline}>{siteConfig.tagline}</p>
        <div className={styles.actions}>
          <Link
            className="button button--primary button--lg"
            to="/docs/getting-started/">
            Get started
          </Link>
          <Link
            className="button button--outline button--secondary button--lg"
            href={repositoryUrl}>
            GitHub
          </Link>
        </div>
      </div>
    </header>
  );
}

const features = [
  {
    href: '/docs/guides/search/',
    title: 'Find the right component',
    description:
      'Search registry components and tokens, then inspect exact snapshot-pinned details.',
  },
  {
    href: '/docs/guides/scan/',
    title: 'Measure adoption',
    description:
      'Scan a repository for design-system usage and produce deterministic reports.',
  },
  {
    href: '/docs/skills/',
    title: 'Work with agents',
    description:
      'Choose a safe, task-focused Wax workflow for your coding agent.',
  },
];

function Feature({href, title, description}) {
  return (
    <Link className={styles.feature} to={href}>
      <h3>{title}</h3>
      <p>{description}</p>
    </Link>
  );
}

export default function Home() {
  const {siteConfig} = useDocusaurusContext();

  return (
    <Layout
      title="Design system coverage analytics"
      description={siteConfig.tagline}>
      <main>
        <HomepageHeader />
        <section
          className={styles.features}
          aria-labelledby="wax-capabilities">
          <div className="container">
            <h2 id="wax-capabilities">What do you want to do?</h2>
            <div className={styles.featureGrid}>
              {features.map((feature) => (
                <Feature key={feature.href} {...feature} />
              ))}
            </div>
          </div>
        </section>
      </main>
    </Layout>
  );
}
