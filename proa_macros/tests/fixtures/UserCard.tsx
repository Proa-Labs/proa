import React from 'react';

interface UserCardProps {
  name: string;
  email: string;
  avatar: string;
  tags: string[];
  metadata?: Record<string, string>;
}

export default function UserCard({ name, email, avatar, tags, metadata }: UserCardProps) {
  return (
    <div className="user-card">
      <img src={avatar} alt={name} />
      <h2>{name}</h2>
      <p>{email}</p>
      <ul>
        {tags.map(tag => <li key={tag}>{tag}</li>)}
      </ul>
    </div>
  );
}
