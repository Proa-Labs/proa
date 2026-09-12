import React from 'react';

interface AccordionProps {
  title: string;
  defaultOpen?: boolean;
  children: React.ReactNode;
}

export default function Accordion({ title, defaultOpen, children }: AccordionProps) {
  const [open, setOpen] = React.useState(defaultOpen ?? false);

  return (
    <div className="accordion">
      <button onClick={() => setOpen(!open)}>
        {title}
      </button>
      {open && <div className="accordion-content">{children}</div>}
    </div>
  );
}
