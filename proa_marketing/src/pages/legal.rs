// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::data::{routes, site};

use axum::response::Html;
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;

use crate::layout::{render_page, PageMeta};

pub async fn privacy_handler() -> Html<String> {
    render_page(
        PageMeta {
            title: "Privacy Policy - Proa",
            description: "Privacy policy for the Proa website, documentation, blog, and related public pages.",
            path: routes::PRIVACY,
        },
        LegalPage {
            eyebrow: "Privacy",
            title: "Privacy Policy",
            intro: const_format::concatcp!("This Privacy Policy explains how ", site::ORGANIZATION_NAME, " handles information when you use Proa websites, documentation, downloads, playgrounds, search, demos, waitlists, and related public pages."),
            sections: PRIVACY_SECTIONS,
            analytics_disclosure: true,
        },
    )
}

pub async fn terms_handler() -> Html<String> {
    render_page(
        PageMeta {
            title: "Terms and Conditions - Proa",
            description: "Terms and conditions for using the Proa website, documentation, blog, and related public pages.",
            path: routes::TERMS,
        },
        LegalPage {
            eyebrow: "Terms",
            title: "Terms and Conditions",
            intro: "These Terms and Conditions govern access to and use of Proa websites, documentation, downloads, playgrounds, demos, benchmarks, and related public pages. Separate written agreements or repository licenses govern paid products, source code, services, support, and commercial licenses.",
            sections: TERMS_SECTIONS,
            analytics_disclosure: false,
        },
    )
}

struct LegalPage {
    eyebrow: &'static str,
    title: &'static str,
    intro: &'static str,
    sections: &'static [LegalSection],
    analytics_disclosure: bool,
}

struct LegalSection {
    title: &'static str,
    body: &'static [&'static str],
}

const PRIVACY_SECTIONS: &[LegalSection] = &[
    LegalSection {
        title: "Scope",
        body: &[
            "This policy applies to information we collect through Proa public websites and related online services that link to this policy.",
            "This policy does not replace separate product, enterprise, support, data processing, or commercial license agreements. If a separate agreement applies, that agreement controls for the covered product or service.",
        ],
    },
    LegalSection {
        title: "Information you provide",
        body: &[
            "We may collect information you provide directly, such as your name, email address, company, role, billing or purchasing details, support requests, feedback, survey responses, and any other information you choose to send us.",
            "If you use a waitlist, download flow, playground, demo, contact form, or support channel, we may collect the information submitted through that interaction.",
        ],
    },
    LegalSection {
        title: "Information collected automatically",
        body: &[
            "When you use the site, we may collect technical and usage information such as IP address, browser type, device information, operating system, pages viewed, referring URLs, timestamps, approximate location derived from IP address, error logs, and request metadata.",
            "We may use cookies, local storage, logs, and similar technologies for site preferences, analytics, security, abuse prevention, performance measurement, and operation of interactive features. For example, the site may use local storage to remember theme preferences.",
        ],
    },
    LegalSection {
        title: "How we use information",
        body: &[
            "We use information to operate, maintain, secure, and improve Proa; provide documentation, downloads, demos, search, and support; respond to requests; communicate about Proa; analyze usage and performance; debug errors; prevent fraud or abuse; enforce terms; and comply with legal or contractual obligations.",
            "We may use aggregated or de-identified information to understand adoption, improve developer experience, and publish or discuss high-level product metrics.",
        ],
    },
    LegalSection {
        title: "How we share information",
        body: &[
            "We may share information with vendors and service providers that help us host, secure, analyze, deliver, support, or operate Proa websites and services. These providers are expected to use information only to provide services to us.",
            "We may disclose information if required by law, legal process, or government request; to protect rights, safety, and security; to investigate misuse; or in connection with a merger, financing, acquisition, reorganization, or sale of assets.",
            "We do not sell personal information. We also do not knowingly share personal information for cross-context behavioral advertising unless we provide any legally required notice and choice.",
        ],
    },
    LegalSection {
        title: "Retention",
        body: &[
            "We keep personal information for as long as reasonably needed for the purposes described in this policy, including to provide services, maintain records, resolve disputes, enforce agreements, improve security, and satisfy legal obligations.",
            "Retention periods vary based on the type of information, how it is used, and applicable legal or operational requirements.",
        ],
    },
    LegalSection {
        title: "Your choices and rights",
        body: &[
            "Depending on where you live, you may have rights to request access, correction, deletion, portability, restriction, objection, or other handling of personal information. These rights may be subject to limits under applicable law.",
            const_format::concatcp!("You can contact us at ", site::PRIVACY_EMAIL, " to make a request. We may need to verify your request before responding."),
            "You can control cookies and similar technologies through your browser settings, but some site features may not work as intended if they are disabled.",
        ],
    },
    LegalSection {
        title: "Security",
        body: &[
            "We use reasonable administrative, technical, and organizational safeguards designed to protect information. No method of transmission or storage is completely secure, so we cannot guarantee absolute security.",
        ],
    },
    LegalSection {
        title: "International users",
        body: &[
            "Proa is operated from the United States. If you access the site from outside the United States, information may be processed in the United States or other jurisdictions where we or our service providers operate.",
        ],
    },
    LegalSection {
        title: "Children",
        body: &[
            "Proa is not directed to children under 13, and we do not knowingly collect personal information from children under 13. If you believe a child has provided personal information to us, contact us so we can review and delete it as appropriate.",
        ],
    },
    LegalSection {
        title: "Changes to this policy",
        body: &[
            "We may update this policy from time to time. The version published on this page is the current version.",
            "If we make material changes, we will provide notice as appropriate under the circumstances.",
        ],
    },
    LegalSection {
        title: "Contact",
        body: &[
            const_format::concatcp!("For privacy questions or requests, contact us at ", site::PRIVACY_EMAIL, "."),
        ],
    },
];

const TERMS_SECTIONS: &[LegalSection] = &[
    LegalSection {
        title: "Acceptance",
        body: &[
            "By accessing or using the Proa public website, documentation, downloads, playgrounds, demos, benchmarks, or related materials, you agree to these terms.",
            "If you do not agree, do not use the public site or materials.",
        ],
    },
    LegalSection {
        title: "Public site and materials",
        body: &[
            "You may use the public website, documentation, and blog for lawful evaluation, learning, and informational purposes.",
            "Examples, benchmarks, documentation, blog posts, playgrounds, and previews may be incomplete, experimental, or subject to change. They are provided to help developers understand Proa and are not a commitment to deliver any feature, price, service level, or roadmap item.",
        ],
    },
    LegalSection {
        title: "Accounts, downloads, and access",
        body: &[
            "Some features may require an account, token, download link, invite, or other access credential. You are responsible for keeping credentials secure and for activity under your credentials.",
            "We may suspend or limit access to any public feature, download, playground, demo, or preview if we believe it is being misused, creates risk, violates these terms, or may harm Proa, users, or third parties.",
        ],
    },
    LegalSection {
        title: "Acceptable use",
        body: &[
            "You may not misuse the site or materials; interfere with operation; bypass access controls; probe, scan, or test systems without permission; introduce malware; overload infrastructure; scrape in a way that harms service operation; attempt unauthorized access; or use the site to violate applicable law or third-party rights.",
            "You may not remove proprietary notices, misrepresent affiliation with Proa, use Proa marks in a confusing way, or use public materials to deceive users.",
        ],
    },
    LegalSection {
        title: "Intellectual property",
        body: &[
            const_format::concatcp!("The website, documentation, examples, logos, text, and other materials are owned by ", site::ORGANIZATION_NAME, " or its licensors unless otherwise stated."),
            "No rights are granted except as expressly provided in these terms, a repository license, or a separate written agreement.",
            "Source code repositories, packages, and downloadable software may be governed by their own license terms. If those terms conflict with these website terms for that software, the software-specific terms control.",
        ],
    },
    LegalSection {
        title: "Feedback",
        body: &[
            const_format::concatcp!("If you provide feedback, ideas, bug reports, suggestions, or other comments, you grant ", site::ORGANIZATION_NAME, " a non-exclusive, worldwide, royalty-free, irrevocable license to use, reproduce, modify, publish, and otherwise exploit that feedback without restriction or compensation."),
            "Do not send confidential information unless we have a separate written agreement covering it.",
        ],
    },
    LegalSection {
        title: "Third-party services and links",
        body: &[
            "The site may link to third-party websites, repositories, package registries, cloud services, payment providers, documentation, or community resources. We are not responsible for third-party content, services, policies, or practices.",
            "Third-party names, logos, and trademarks are the property of their respective owners and are used only for identification, compatibility, comparison, or illustrative purposes unless otherwise stated.",
        ],
    },
    LegalSection {
        title: "No warranties",
        body: &[
            "The public website and materials are provided as-is and as-available.",
            const_format::concatcp!("To the maximum extent permitted by law, ", site::ORGANIZATION_NAME, " disclaims warranties of merchantability, fitness for a particular purpose, non-infringement, availability, and accuracy."),
        ],
    },
    LegalSection {
        title: "Limitation of liability",
        body: &[
            const_format::concatcp!("To the maximum extent permitted by law, ", site::ORGANIZATION_NAME, " will not be liable for indirect, incidental, special, consequential, exemplary, or punitive damages arising from use of the public website or materials."),
            const_format::concatcp!("To the maximum extent permitted by law, ", site::ORGANIZATION_NAME, "' total liability for claims arising from or relating to the public website or materials will not exceed one hundred U.S. dollars."),
        ],
    },
    LegalSection {
        title: "Indemnity",
        body: &[
            const_format::concatcp!("You agree to defend, indemnify, and hold harmless ", site::ORGANIZATION_NAME, " from claims, liabilities, damages, losses, and expenses arising from your misuse of the public website or materials, your violation of these terms, or your violation of applicable law or third-party rights."),
        ],
    },
    LegalSection {
        title: "Changes to these terms",
        body: &[
            "We may update these terms from time to time. Continued use of the website after an update means you accept the revised terms.",
        ],
    },
    LegalSection {
        title: "Governing law",
        body: &[
            "These terms are governed by the laws of the State of Delaware, without regard to conflict-of-law principles.",
            "Subject to any separate written agreement, disputes relating to these terms or the public website will be brought in the state or federal courts located in Delaware, and each party consents to personal jurisdiction and venue there.",
        ],
    },
    LegalSection {
        title: "Contact",
        body: &[
            const_format::concatcp!("For questions about these terms, contact us at ", site::LEGAL_EMAIL, "."),
        ],
    },
];

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for LegalPage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <section class="px-4 py-16 sm:px-6 lg:px-8">
                <div class="mx-auto max-w-3xl">
                    <p class="text-xs font-semibold uppercase tracking-[0.24em] text-[#dd7627]">
                        {self.eyebrow}
                    </p>
                    <h1 class="mt-4 font-display text-4xl font-semibold tracking-[-0.04em] text-zinc-950 dark:text-zinc-50 sm:text-5xl">
                        {self.title}
                    </h1>
                    <p class="mt-6 text-lg leading-8 text-zinc-700 dark:text-zinc-300">
                        {self.intro}
                    </p>
                    <div class="mt-12 space-y-10">
                        {if self.analytics_disclosure {
                            GoogleAnalyticsDisclosure
                        }}
                        {for section in self.sections {
                            LegalSectionView { section }
                        }}
                    </div>
                </div>
            </section>
        }
        .render(cx)
    }
}

struct GoogleAnalyticsDisclosure;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for GoogleAnalyticsDisclosure {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <section data-google-analytics-disclosure>
                <h2 class="font-display text-2xl font-semibold tracking-[-0.03em] text-zinc-950 dark:text-zinc-50">
                    "Google Analytics"
                </h2>
                <div class="mt-4 space-y-4 text-base leading-8 text-zinc-700 dark:text-zinc-300">
                    <p>
                        "We use Google Analytics to understand visits and interactions on our website. Google Analytics uses cookies and similar technologies to collect information such as pages viewed, referring sites, browser and device details, and interactions. Google processes this information to provide usage reports to "
                        {site::ORGANIZATION}
                        " We disable Google Signals and advertising personalization signals in our website's Analytics configuration."
                    </p>
                    <p>
                        "Learn "
                        <a
                            class="underline underline-offset-4"
                            href="https://policies.google.com/technologies/partner-sites"
                        >
                            "how Google uses information from sites that use its services"
                        </a>
                        ". You can control cookies in your browser or use the "
                        <a
                            class="underline underline-offset-4"
                            href="https://tools.google.com/dlpage/gaoptout"
                        >
                            "Google Analytics opt-out browser add-on"
                        </a>
                        "."
                    </p>
                </div>
            </section>
        }
        .render(cx)
    }
}

struct LegalSectionView<'a> {
    section: &'a LegalSection,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for LegalSectionView<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <section>
                <h2 class="font-display text-2xl font-semibold tracking-[-0.03em] text-zinc-950 dark:text-zinc-50">
                    {self.section.title}
                </h2>
                <div class="mt-4 space-y-4 text-base leading-8 text-zinc-700 dark:text-zinc-300">
                    {for &paragraph in self.section.body {
                        LegalParagraphView { paragraph }
                    }}
                </div>
            </section>
        }
        .render(cx)
    }
}

struct LegalParagraphView {
    paragraph: &'static str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for LegalParagraphView {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <p>{self.paragraph}</p>
        }
        .render(cx)
    }
}
